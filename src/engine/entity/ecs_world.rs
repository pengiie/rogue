use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::{collections::HashSet, ops::Deref};

use rogue_macros::Resource;

use super::{
    scripting::ScriptableEntity, EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
};
use crate::common::dyn_vec::TypeInfo;
use crate::common::freelist::{FreeList, FreeListHandle};
use crate::common::geometry::obb::OBB;
use crate::common::vtable;
use crate::engine::entity::archetype::ComponentArchetype;
use crate::engine::entity::component::{
    Bundle, ComponentBorrowMap, ComponentTypeBorrow, GameComponent, GameComponentContext,
};
use crate::engine::entity::query::{
    Query, QueryBorrow, QueryItem, QueryItemRef, QueryMany, QueryOne,
};
use crate::engine::event::{EventReader, Events};
use crate::engine::resource::ResMut;
use crate::{
    engine::{
        graphics::camera::{Camera, MainCamera},
        physics::{collider::Colliders, rigid_body::RigidBody, transform::Transform},
        system::SystemParam,
        voxel::{voxel::VoxelModelImpl, voxel_world::VoxelWorld},
    },
    game::entity::player::Player,
};

pub type Entity = FreeListHandle<EntityInfo>;

pub struct EventEntityDespawn(pub Entity);

#[derive(Debug)]
pub struct EntityInfo {
    pub components: Vec<TypeInfo>,
    // Index of the archetype
    pub archetype_ptr: usize,
    // Index in the archetype
    pub index: usize,
}

#[derive(Resource)]
pub struct ECSWorld {
    pub archetypes: Vec<ComponentArchetype>,
    // Makes it easier to see what archetypes are required for a type.
    pub component_archetypes: HashMap<TypeId, Vec</*archetype_index*/ usize>>,
    pub entities: FreeList<EntityInfo>,
    pub game_component_vtables: HashMap<TypeId, *const ()>,
    pub despawn_event_reader: EventReader<EventEntityDespawn>,
}

impl ECSWorld {
    pub fn new() -> ECSWorld {
        let mut ecs = ECSWorld {
            archetypes: Vec::new(),
            component_archetypes: HashMap::new(),
            entities: FreeList::new(),
            game_component_vtables: HashMap::new(),
            despawn_event_reader: EventReader::new(),
        };

        // Makes these components cloneable and serializable in the project.
        // Would be nice to dynamically register sure, but that is overhead on every call and makes
        // finding which components are being serialized and persisted for a project easier.
        // Grep match words: "register game components", "game component register".
        ecs.register_game_component::<GameEntity>();
        ecs.register_game_component::<EntityParent>();
        ecs.register_game_component::<EntityChildren>();
        ecs.register_game_component::<Transform>();
        ecs.register_game_component::<RenderableVoxelEntity>();
        ecs.register_game_component::<Camera>();
        ecs.register_game_component::<RigidBody>();
        ecs.register_game_component::<Colliders>();

        ecs
    }

    pub fn run_queued_despawns(ecs_world: ResMut<ECSWorld>, events: ResMut<Events>) {}

    fn register_game_component<C: GameComponent + 'static>(&mut self) {
        let type_id = std::any::TypeId::of::<C>();
        // Technically there can be two different vtable ptrs for the same type due to something
        // about codegen units, but that doesn't matter here since semantically there is no
        // difference so ignore duplicates.
        if self.game_component_vtables.contains_key(&type_id) {
            return;
        }

        // Safety: We never access the contents of the pointer, only extracting the vtable, so
        // should be okay right? Use `without_provenance_mut` since this ptr isn't actually
        // associated with a memory allocation.
        let null = unsafe { NonNull::new_unchecked(std::ptr::without_provenance_mut::<C>(0x1234)) };
        let dyn_ref = unsafe { null.as_ref() } as &dyn GameComponent;
        // Safety: This reference is in fact a dyn ref.
        let vtable_ptr = unsafe { vtable::get_vtable_ptr(dyn_ref as &dyn GameComponent) };
        self.game_component_vtables.insert(type_id, vtable_ptr);
    }

    pub fn get_or_create_archetype_static<'a>(
        archetypes: &'a mut Vec<ComponentArchetype>,
        component_archetype: &mut HashMap<TypeId, Vec<usize>>,
        type_infos: Vec<TypeInfo>,
    ) -> (usize, &'a mut ComponentArchetype) {
        assert!(type_infos.is_sorted());
        {
            if let Some((i, archetype)) =
                archetypes
                    .iter_mut()
                    .enumerate()
                    .find_map(|(i, archetype)| {
                        (archetype.types == type_infos)
                            .then_some((i, std::ptr::from_mut(archetype)))
                    })
            {
                // Safety, we are simply passing as a ptr to skip the borrow checker, it is
                // assuming the borrow of archetype past the if let block which is incorrect.
                return (i, unsafe { &mut *archetype });
            }
        }

        let last_index = archetypes.len();
        for type_info in &type_infos {
            // Should be a unique index since this is the first time creating this archetype and
            // removal isn't implemented yet.
            component_archetype
                .entry(type_info.type_id)
                .or_default()
                .push(last_index);
        }
        archetypes.push(ComponentArchetype::new(type_infos));
        return (last_index, &mut archetypes[last_index]);
    }

    pub fn get_or_create_archetype(
        &mut self,
        type_infos: Vec<TypeInfo>,
    ) -> (usize, &mut ComponentArchetype) {
        return Self::get_or_create_archetype_static(
            &mut self.archetypes,
            &mut self.component_archetypes,
            type_infos,
        );
    }

    pub fn spawn<B: Bundle + 'static>(&mut self, bundle: B) -> Entity {
        let entity = self.spawn_raw(bundle.type_info());
        std::mem::forget(bundle);
        return entity;
    }

    pub fn spawn_raw(&mut self, mut data: Vec<(TypeInfo, *const u8)>) -> Entity {
        data.sort_by(|(type_info_a, _), (type_info_b, _)| type_info_a.cmp(type_info_b));
        let type_infos = data
            .iter()
            .map(|(type_info, _)| type_info.clone())
            .collect::<Vec<_>>();
        let type_ids = type_infos
            .iter()
            .map(|type_info| type_info.type_id())
            .collect::<Vec<_>>();

        let entity_id = self.entities.next_free_handle();
        let (archetype_ptr, archetype) = self.get_or_create_archetype(type_infos.clone());
        let archetype_index = archetype.insert(entity_id, data);

        let pushed_entity_id = self.entities.push(EntityInfo {
            components: type_infos,
            archetype_ptr,
            index: archetype_index,
        });
        assert_eq!(entity_id, pushed_entity_id);

        return entity_id;
    }

    pub fn insert_one<C: 'static>(
        &mut self,
        entity_id: Entity,
        mut component: C,
    ) -> anyhow::Result<()> {
        let component_type_id = std::any::TypeId::of::<C>();

        let entity_info = self.entities.get_mut(entity_id).unwrap();
        assert!(entity_info.components.is_sorted());
        let old_archetype = &mut self.archetypes[entity_info.archetype_ptr];
        let old_type_infos = entity_info.components.clone();

        // Check if this component is already in the current entity's archetype.
        if old_type_infos
            .into_iter()
            .find(|type_info| type_info.type_id == component_type_id)
            .is_some()
        {
            // Replace the old component.
            let type_info = TypeInfo::new::<C>();
            let component_ref = old_archetype.get_mut::<C>(&type_info, entity_info.index as usize);
            *component_ref = component;
            return Ok(());
        }

        // Get or create new archetype and move entity components to it.
        let mut new_type_infos = old_archetype.types.clone();
        new_type_infos.push(TypeInfo::new::<C>());
        new_type_infos.sort();
        let new_type_ids = new_type_infos
            .iter()
            .map(|type_info| type_info.type_id)
            .collect::<Vec<_>>();

        let mut new_ptrs = old_archetype.take_raw(entity_info.index as usize);
        let new_type_index = new_type_ids
            .iter()
            .position(|ty| *ty == component_type_id)
            .unwrap();
        let (new_archetype_ptr, mut new_archetype) = Self::get_or_create_archetype_static(
            &mut self.archetypes,
            &mut self.component_archetypes,
            new_type_infos.clone(),
        );

        let component_ptr = std::ptr::from_mut(&mut component);
        new_ptrs.insert(new_type_index, component_ptr as *mut u8);
        std::mem::forget(component);

        // Safety: We used the same type infos as the old archetype, and insert the new
        // component in the new type info's location. All the pointers are also valid since
        // `old_archetype` and `new_archetype` must be disjoint due to differing type ids. And the
        // `old_archetype` is not mutated after getting the data ptrs.
        entity_info.index = unsafe { new_archetype.insert_raw(entity_id, new_ptrs) };
        entity_info.archetype_ptr = new_archetype_ptr;
        entity_info.components = new_type_infos;

        return Ok(());
    }

    /// Clones any entity in the world with a GameEntity component. Only clones components which
    /// implement the `GameComponent` trait. This also preserves the same entity ids to keep
    /// references coherent.
    pub fn clone_game_entities(&self, mut ctx: GameComponentContext) -> ECSWorld {
        let ctx = &mut ctx;
        let mut new_world = ECSWorld::new();
        new_world.game_component_vtables = self.game_component_vtables.clone();

        for (entity, entity_info) in self.entities.iter_with_handle() {
            if entity_info
                .components
                .iter()
                .find(|ty| ty.type_id == TypeId::of::<GameEntity>())
                .is_none()
            {
                continue;
            }

            let entity_game_components = entity_info
                .components
                .iter()
                .filter_map(|type_info| {
                    self.game_component_vtables
                        .contains_key(&type_info.type_id)
                        .then_some(type_info.clone())
                })
                .collect::<Vec<_>>();
            assert!(entity_game_components.is_sorted());

            let src_archetype = self.find_archetype(entity);
            let src_data = entity_game_components
                .iter()
                .map(|type_info| unsafe {
                    (
                        type_info,
                        src_archetype.get_raw(type_info, entity_info.index).as_ptr(),
                    )
                })
                .collect::<Vec<_>>();
            let cloned_data = src_data
                .iter()
                .map(|(type_info, src_data)| {
                    let game_component_vtable =
                        self.game_component_vtables.get(&type_info.type_id).unwrap();
                    let game_component_ptr = unsafe {
                        std::mem::transmute::<(*const u8, *const ()), *const dyn GameComponent>((
                            *src_data as *const u8,
                            *game_component_vtable as *const (),
                        ))
                    };
                    let game_component = unsafe { game_component_ptr.as_ref().unwrap() };
                    // Safety: We free the pointers after the data is copied to the new archetype.
                    let clone_dst_layout = type_info.layout(1);
                    let clone_dst = unsafe { std::alloc::alloc(clone_dst_layout) };
                    assert!(!clone_dst.is_null());
                    game_component.clone_component(ctx, clone_dst);
                    (clone_dst, clone_dst_layout)
                })
                .collect::<Vec<_>>();

            let (archetype_ptr, archetype) =
                new_world.get_or_create_archetype(entity_game_components.clone());
            let archetype_index = unsafe {
                archetype.insert_raw(
                    entity,
                    cloned_data.iter().map(|(ptr, _)| *ptr).collect::<Vec<_>>(),
                )
            };

            for (cloned_ptr, cloned_dst_layout) in cloned_data {
                // Safety: We check it is not null, and it is allocated above.
                unsafe { std::alloc::dealloc(cloned_ptr, cloned_dst_layout) };
            }

            new_world.entities.insert_in_place(
                entity,
                EntityInfo {
                    components: entity_game_components,
                    archetype_ptr,
                    index: archetype_index,
                },
            );
        }

        return new_world;
    }

    pub fn query_mut<Q: Query>(&mut self) -> QueryBorrow<Q> {
        return self.query();
    }

    pub fn query<Q: Query>(&self) -> QueryBorrow<Q> {
        return QueryBorrow::new(self);
    }

    pub fn query_one<Q: Query>(&self, entity: Entity) -> QueryOne<Q> {
        QueryOne::<Q>::new(self, entity)
    }

    pub fn get<'a, C: QueryItem + 'static>(&'a self, entity: Entity) -> anyhow::Result<C::Ref<'a>> {
        let archetype = self.find_archetype(entity);
        if !archetype.has_type_id(C::item_type_id()) {
            anyhow::bail!("Entity does not have type C");
        }

        let entity_info = self.entities.get(entity).unwrap();
        let r = C::Ref::<'a>::create_ref(archetype, entity_info.index);
        Ok(r)
    }

    pub fn find_archetype(&self, entity: Entity) -> &ComponentArchetype {
        let entity_info = self.entities.get(entity).unwrap();
        assert!(entity_info.components.is_sorted());
        return &self.archetypes[entity_info.archetype_ptr];
    }

    pub fn query_many_mut<Q: Query, const C: usize>(
        &self,
        entities: [Entity; C],
    ) -> QueryMany<Q, C> {
        QueryMany::new(self, entities)
    }

    pub fn remove_one<C: 'static>(&mut self, entity: Entity) {
        todo!();
    }

    // Gets the minimum OBB of the entity's voxel model.
    pub fn get_entity_obb(&self, entity: Entity, voxel_world: &VoxelWorld) -> Option<OBB> {
        let mut query = self.query_one::<(&Transform, &RenderableVoxelEntity)>(entity);
        let Some((mut local_transform, mut renderable)) = query.get() else {
            return None;
        };
        if renderable.is_null() {
            return None;
        }

        let world_transform = self.get_world_transform(entity, &local_transform);
        let voxel_model = voxel_world
            .registry
            .get_dyn_model(renderable.voxel_model_id_unchecked());
        return Some(world_transform.as_voxel_model_obb(voxel_model.length()));
    }

    //pub fn clone_game_entities(&mut self) -> ECSWorld {
    //    let mut new = ECSWorld::new();
    //    for (
    //        entity,
    //        (
    //            game_entity,
    //            transform,
    //            parent,
    //            children,
    //            renderable,
    //            camera,
    //            scriptable,
    //            rigid_body,
    //            colliders,
    //        ),
    //    ) in self.query_mut::<(
    //        &GameEntity,
    //        &Transform,
    //        Option<&EntityParent>,
    //        Option<&EntityChildren>,
    //        Option<&RenderableVoxelEntity>,
    //        Option<&Camera>,
    //        Option<&ScriptableEntity>,
    //        Option<&RigidBody>,
    //        Option<&Colliders>,
    //    )>() {
    //        // Must use spawn_at so EntityParent and EntityChildren stay correct.
    //        new.spawn_at(entity, (game_entity.clone(), transform.clone()));
    //        if let Some(parent) = parent {
    //            new.insert_one(entity, parent.clone());
    //        }
    //        if let Some(children) = children {
    //            new.insert_one(entity, children.clone());
    //        }
    //        if let Some(renderable) = renderable {
    //            new.insert_one(entity, renderable.clone());
    //        }
    //        if let Some(camera) = camera {
    //            new.insert_one(entity, camera.clone());
    //        }
    //        if let Some(scriptable) = scriptable {
    //            new.insert_one(entity, scriptable.clone());
    //        }
    //        if let Some(rigid_body) = rigid_body {
    //            new.insert_one(entity, rigid_body.clone());
    //        }
    //        if let Some(colliders) = colliders {
    //            new.insert_one(entity, colliders.clone());
    //        }
    //    }

    //    return new;
    //}

    pub fn player_query<'a, Q: Query>(&'a self) -> PlayerQuery<Q> {
        PlayerQuery::new(self.query::<Q>().with::<(Player,)>() as QueryBorrow<'a, Q>)
    }

    pub fn contains(&self, entity: Entity) -> bool {
        todo!()
    }

    pub fn despawn(&mut self, entity: Entity) {
        todo!()
    }

    pub fn get_main_camera(&self, main_camera: &MainCamera) -> QueryOne<'_, (&Transform, &Camera)> {
        self.query_one::<(&Transform, &Camera)>(
            main_camera
                .camera()
                .expect("Main camera has not been set yet."),
        )
    }

    pub fn set_parent(&mut self, entity: Entity, parent: Entity) {
        self.insert_one(entity, EntityParent::new(parent));
        let contains_children = self.get::<&mut EntityChildren>(parent).is_ok();
        if contains_children {
            let mut children = self.get::<&mut EntityChildren>(parent).unwrap();
            children.children.insert(entity);
        } else {
            let mut children = HashSet::new();
            children.insert(entity);
            self.insert_one(parent, EntityChildren { children });
        }
    }

    pub fn get_world_transform(
        &self,
        entity: Entity,
        entity_local_transform: &Transform,
    ) -> Transform {
        let mut curr_transform = entity_local_transform.clone();

        let mut curr_parent = self.get::<&EntityParent>(entity);
        while let Ok(parent) = curr_parent {
            let Ok(parent_transform) = self.get::<&Transform>(parent.parent) else {
                break;
            };
            curr_transform.position =
                (parent_transform.rotation * curr_transform.position) + parent_transform.position;
            curr_transform.rotation = parent_transform.rotation * curr_transform.rotation;
            curr_transform.scale = curr_transform.scale.component_mul(&parent_transform.scale);
            curr_parent = self.get::<&EntityParent>(parent.parent);
        }

        return curr_transform;
    }
}

pub struct PlayerQuery<'a, Q: Query>(QueryBorrow<'a, Q>);

impl<'a, Q: Query> PlayerQuery<'a, Q> {
    pub fn new(query: QueryBorrow<'a, Q>) -> Self {
        Self(query)
    }

    pub fn player<'b>(&'b mut self) -> (Entity, Q::Item<'b>) {
        if self.0.iter().len() > 1 {
            panic!("More than one player spawned?");
        }
        self.0.iter().next().expect("Player was not spawned.")
    }

    pub fn try_player<'b>(&'b mut self) -> Option<(Entity, Q::Item<'b>)> {
        if self.0.iter().len() > 1 {
            panic!("More than one player spawned?");
        }
        self.0.iter().next()
    }
}
