use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::{collections::HashSet, ops::Deref};

use rogue_macros::Resource;
use serde::ser::{SerializeSeq, SerializeStruct};
use uuid::Uuid;

use super::{
    EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity, scripting::ScriptableEntity,
};
use crate::animation::animator::Animator;
use crate::asset::repr::game_entity::{WorldGameComponentAsset, WorldGameEntityAsset};
use crate::asset::repr::project::ProjectSceneDeserializeContext;
use crate::common::dyn_vec::TypeInfo;
use crate::common::freelist::{FreeList, FreeListHandle};
use crate::common::geometry::obb::OBB;
use crate::common::vtable;
use crate::entity::archetype::ComponentArchetype;
use crate::entity::component::{
    Bundle, ComponentBorrowMap, ComponentTypeBorrow, GameComponent, GameComponentCloneContext,
    GameComponentDeserializeContext, GameComponentDeserializeFnPtr, GameComponentMethods,
    GameComponentMethodsVtablePtr, GameComponentSerializeContext, GameComponentType,
    RawComponentRef,
};
use crate::entity::ecs_world;
use crate::entity::query::{Query, QueryBorrow, QueryItem, QueryItemRef, QueryMany, QueryOne};
use crate::event::{EventReader, Events};
use crate::graphics::camera::{Camera, MainCamera};
use crate::physics::collider_component::EntityColliders;
use crate::physics::{rigid_body::RigidBody, transform::Transform};
use crate::resource::ResMut;
use crate::system::SystemParam;
use crate::voxel::voxel::VoxelModelImplMethods;
use crate::voxel::voxel_registry::VoxelModelRegistry;

pub type Entity = FreeListHandle<EntityInfo>;

#[derive(Debug, Clone)]
pub enum EntityCommandEvent {
    Despawn {
        entity: Entity,
        despawn_children: bool,
    },
    /// See ECSWorld::set_parent().
    SetParent {
        parent: Option<Entity>,
        child: Entity,
        modify_transform: bool,
    },
}

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
    pub game_components: HashMap<TypeId, GameComponentType>,
    pub game_component_names: HashMap</*GameComponent::NAME*/ String, TypeId>,
    pub entity_event_reader: EventReader<EntityCommandEvent>,
}

impl ECSWorld {
    /// Creates an instance of ECSWorld which should be registered with any
    /// non-engine component types before serialization.
    pub fn new() -> ECSWorld {
        let mut ecs = ECSWorld {
            archetypes: Vec::new(),
            component_archetypes: HashMap::new(),
            entities: FreeList::new(),
            game_components: HashMap::new(),
            game_component_names: HashMap::new(),
            entity_event_reader: EventReader::new(),
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
        ecs.register_game_component::<EntityColliders>();
        ecs.register_game_component::<Animator>();

        ecs
    }

    pub fn construct_and_insert_game_component(
        &mut self,
        entity_id: Entity,
        game_component: TypeId,
    ) {
        let component_ptr = self.construct_game_component(game_component);
        let game_component = self.game_components.get(&game_component).expect("Tried to construct and insert a game component for a type that is not registered as a game component.");
        let type_info = &game_component.type_info.clone();
        // Safety: component_ptr is allocated with the correct memory layout for this type.
        unsafe {
            self.insert_one_raw(entity_id, &type_info, component_ptr)
                .expect("Failed to insert constructed game component into entity.");
        }
    }

    // Returns an owned pointer to that game component, allocated with the correct memory layout.
    pub fn construct_game_component(&self, game_component: TypeId) -> *mut u8 {
        let game_component = self.game_components.get(&game_component).expect("Tried to construct a game component for a type that is not registered as a game component.");
        let ptr = unsafe { std::alloc::alloc(game_component.type_info.layout(1)) };
        if ptr.is_null() {
            panic!("Failed to allocate memory for game component.");
        }
        // Safety: ptr is allocated with the correct memory layout for this type and is not null.
        unsafe { (game_component.construct_fn)(ptr) };
        return ptr;
    }

    pub fn get_constructible_game_components(&self) -> Vec<TypeId> {
        let mut constructible = Vec::new();
        for (type_id, game_component) in &self.game_components {
            if game_component.is_constructible {
                constructible.push(*type_id);
            }
        }
        return constructible;
    }

    /// Returns a serde serializable object which holds references to the required data structures
    /// to serialize the world.
    pub fn serialize_world<'a>(
        &'a self,
        ctx: &'a GameComponentSerializeContext<'a>,
    ) -> ECSWorldSerializable<'a> {
        ECSWorldSerializable {
            ecs_world: self,
            ctx,
        }
    }

    pub fn deserialize_world<'a, D: serde::Deserializer<'a>>(
        ctx: &mut GameComponentCloneContext<'_>,
        ser: D,
    ) -> Result<Self, D::Error> {
        todo!();
    }

    pub fn handle_entity_commands(mut ecs_world: ResMut<ECSWorld>, events: ResMut<Events>) {
        let mut ecs_world = &mut ecs_world as &mut ECSWorld;
        let entity_events = ecs_world
            .entity_event_reader
            .read(&events)
            .map(|event| event.clone())
            .collect::<Vec<_>>();
        for event in entity_events {
            match event {
                EntityCommandEvent::Despawn {
                    entity,
                    despawn_children,
                } => {
                    ecs_world.despawn(entity, despawn_children);
                }
                EntityCommandEvent::SetParent {
                    parent,
                    child,
                    modify_transform,
                } => {
                    ecs_world.set_parent(child, parent, modify_transform);
                }
            }
        }
    }

    pub fn register_game_component<C: GameComponent + 'static>(&mut self) {
        let type_id = std::any::TypeId::of::<C>();
        // Technically there can be two different vtable ptrs for the same type due to something
        // about codegen units, but that doesn't matter here since semantically there is no
        // difference so ignore duplicates.
        if self.game_components.contains_key(&type_id) {
            return;
        }

        // Safety: We never access the contents of the pointer, only extracting the vtable, so
        // should be okay right? Use `without_provenance_mut` since this ptr isn't actually
        // associated with a memory allocation.
        let null = unsafe { NonNull::new_unchecked(std::ptr::without_provenance_mut::<C>(0x1234)) };
        let dyn_ref = unsafe { null.as_ref() } as &dyn GameComponentMethods;
        // Safety: This reference is in fact a dyn ref.
        let vtable_ptr = unsafe { vtable::get_vtable_ptr(dyn_ref as &dyn GameComponentMethods) };
        self.game_components.insert(
            type_id,
            GameComponentType {
                type_info: TypeInfo::new::<C>(),
                component_name: C::NAME.to_owned(),
                is_constructible: C::is_constructible(),
                construct_fn: C::construct_component,
                deserialize_fn: C::deserialize_component,
                methods_vtable_ptr: vtable_ptr,
            },
        );

        let old = self
            .game_component_names
            .insert(C::NAME.to_owned(), type_id);
        assert!(
            old.is_none(),
            "{} game component has a duplicate GameComponent::NAME with another already registered component.",
            std::any::type_name::<C>()
        );
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
        let entity = self.spawn_raw(unsafe { bundle.type_info() });
        std::mem::forget(bundle);
        return entity;
    }

    /// Takes ownership of the given raw data and spawns an entity with it.
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

        assert!(type_infos.is_sorted());
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
        let type_info = TypeInfo::new::<C>();
        let component_ptr = std::ptr::from_mut(&mut component);
        // Safety: component is forgotten so it is taken ownership of and the type info
        // and ptr are from the same type.
        unsafe { self.insert_one_raw(entity_id, &type_info, component_ptr as *mut u8) }?;
        std::mem::forget(component);
        Ok(())
    }

    /// Safety: Expects `component` to be allocated with the provided type info's layout and
    /// ownership moved to this ECSWorld.
    pub unsafe fn insert_one_raw(
        &mut self,
        entity_id: Entity,
        type_info: &TypeInfo,
        component_ptr: *mut u8,
    ) -> anyhow::Result<()> {
        let entity_info = self.entities.get_mut(entity_id).unwrap();
        assert!(entity_info.components.is_sorted());
        let old_archetype = &mut self.archetypes[entity_info.archetype_ptr];
        let old_type_infos = entity_info.components.clone();

        // Check if this component is already in the current entity's archetype.
        if old_type_infos
            .into_iter()
            .find(|old_type_info| old_type_info.type_id == type_info.type_id)
            .is_some()
        {
            // Replace the old component.
            // Safety: Safe by expectations of calling this function.
            old_archetype.replace_component_raw(
                entity_info.index as usize,
                type_info,
                component_ptr,
            );
            return Ok(());
        }

        // Get or create new archetype and move entity components to it.
        let mut new_type_infos = old_archetype.types.clone();
        new_type_infos.push(type_info.clone());
        new_type_infos.sort();
        let new_type_ids = new_type_infos
            .iter()
            .map(|type_info| type_info.type_id)
            .collect::<Vec<_>>();
        log::debug!(
            "Inserting component {:?} into entity {:?}, moving from archetype types {:?} to {:?}",
            type_info.name(),
            entity_id,
            old_archetype
                .types
                .iter()
                .map(|ty| (ty.type_id(), ty.name()))
                .collect::<Vec<_>>(),
            new_type_infos
                .iter()
                .map(|ty| (ty.type_id(), ty.name()))
                .collect::<Vec<_>>()
        );

        let mut new_ptrs = old_archetype.take_raw(entity_info.index as usize);
        let new_type_index = new_type_ids
            .iter()
            .position(|ty| *ty == type_info.type_id)
            .unwrap();
        let (new_archetype_ptr, mut new_archetype) = Self::get_or_create_archetype_static(
            &mut self.archetypes,
            &mut self.component_archetypes,
            new_type_infos.clone(),
        );

        log::debug!(
            "new type index for inserted component {:?} is {}",
            type_info.name(),
            new_type_index
        );
        new_ptrs.insert(new_type_index, component_ptr as *mut u8);

        // Safety: We used the same type infos as the old archetype, and insert the new
        // component in the new type info's location. All the pointers are also valid since
        // `old_archetype` and `new_archetype` must be disjoint due to differing type ids. And the
        // `old_archetype` is not mutated after getting the data ptrs.
        entity_info.index = unsafe { new_archetype.insert_raw(entity_id, new_ptrs) };
        entity_info.archetype_ptr = new_archetype_ptr;
        entity_info.components = new_type_infos;
        log::debug!("Post insert");

        return Ok(());
    }

    fn inherit_game_component_types(&mut self, other: &Self) {
        for (ty, game_component) in &other.game_components {
            self.game_component_names
                .insert(game_component.component_name.clone(), *ty);
            self.game_components.insert(*ty, game_component.clone());
        }
    }

    /// Clones any entity in the world with a GameEntity component. Only clones components which
    /// implement the `GameComponent` trait. This also preserves the same entity ids to keep
    /// references coherent.
    pub fn clone_game_entities(&self, mut ctx: &mut GameComponentCloneContext) -> ECSWorld {
        let mut new_world = ECSWorld::new();
        new_world.inherit_game_component_types(self);

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
                    self.game_components
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
                        src_archetype
                            .get_raw(&type_info.type_id, entity_info.index)
                            .as_ptr(),
                    )
                })
                .collect::<Vec<_>>();
            let cloned_data = src_data
                .iter()
                .map(|(type_info, src_data)| {
                    let game_component_vtable = self
                        .game_components
                        .get(&type_info.type_id)
                        .unwrap()
                        .methods_vtable_ptr;
                    let game_component_ptr = unsafe {
                        std::mem::transmute::<
                            (*const u8, GameComponentMethodsVtablePtr),
                            *const dyn GameComponentMethods,
                        >((*src_data, game_component_vtable))
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

    unsafe fn clone_component(
        src_data: *const u8,
        type_info: &TypeInfo,
        vtable_ptr: GameComponentMethodsVtablePtr,
        ctx: &mut GameComponentCloneContext<'_>,
    ) -> (*mut u8, std::alloc::Layout) {
        let game_component_ptr = unsafe {
            std::mem::transmute::<
                (*const u8, GameComponentMethodsVtablePtr),
                *const dyn GameComponentMethods,
            >((src_data, vtable_ptr))
        };
        let game_component = unsafe { game_component_ptr.as_ref().unwrap() };
        // Safety: We free the pointers after the data is copied to the new archetype.
        let clone_dst_layout = type_info.layout(1);
        let clone_dst = unsafe { std::alloc::alloc(clone_dst_layout) };
        assert!(!clone_dst.is_null());
        game_component.clone_component(ctx, clone_dst);
        (clone_dst, clone_dst_layout)
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

    pub fn get_unchecked<'a>(
        &'a self,
        entity: Entity,
        component_type: TypeId,
    ) -> RawComponentRef<'a> {
        let archetype = self.find_archetype(entity);
        if !archetype.has_type_id(component_type) {
            panic!("Entity does not have the requested component type.");
        }

        let entity_info = self.entities.get(entity).unwrap();
        return RawComponentRef::create_ref(archetype, &component_type, entity_info.index);
    }

    pub fn get<'a, C: QueryItem + 'static>(&'a self, entity: Entity) -> anyhow::Result<C::Ref<'a>> {
        let archetype = self.find_archetype(entity);
        let type_id = C::required_type_id().expect("Should not use Option<Component> for this get, I should probably make this explicit on the type level later.");
        if !archetype.has_type_id(type_id) {
            anyhow::bail!("Entity does not have type C");
        }

        let entity_info = self.entities.get(entity).unwrap();
        let r = C::Ref::<'a>::create_ref(archetype, entity_info.index);
        Ok(r)
    }

    pub fn find_archetype(&self, entity: Entity) -> &ComponentArchetype {
        let entity_info = self.entities.get(entity).unwrap();
        assert!(
            entity_info.components.is_sorted(),
            "Entity {:?} has unsorted components {:?}??",
            entity,
            entity_info.components
        );
        return &self.archetypes[entity_info.archetype_ptr];
    }

    pub fn find_archetype_mut(&mut self, entity: Entity) -> &mut ComponentArchetype {
        let entity_info = self.entities.get(entity).unwrap();
        assert!(entity_info.components.is_sorted());
        return &mut self.archetypes[entity_info.archetype_ptr];
    }

    pub fn query_many_mut<Q: Query, const C: usize>(
        &self,
        entities: [Entity; C],
    ) -> QueryMany<Q, C> {
        QueryMany::new(self, entities)
    }

    pub fn remove_one<C: 'static>(&mut self, entity: Entity) -> C {
        self.try_remove_one(entity)
            .expect("Entity does not have component.")
    }

    pub fn try_remove_one<C: 'static>(&mut self, entity: Entity) -> Option<C> {
        // Safety: We have a mutable reference to self and do not reference the ptr data past our reference.
        let ptr = unsafe { self.try_remove_one_raw(entity, &std::any::TypeId::of::<C>()) };
        // Safety: The ptr returned from `Self::try_remove_one_raw` is an owned ptr and is properly
        // aligned to type C.
        ptr.map(|ptr| unsafe { (ptr as *mut C).read() })
    }

    /// Returns an owned ptr to the removed component data, returns None if the entity doesn't have
    /// the specified component.
    /// Safety: Returned ptr is only valid as long as new data isn't written to this ECSWorld.
    pub unsafe fn try_remove_one_raw(
        &mut self,
        entity: Entity,
        type_id: &TypeId,
    ) -> Option<*mut u8> {
        let entity_info = self
            .entities
            .get_mut(entity)
            .expect("Given entity doesn't exist.");

        let mut new_component_types = entity_info.components.clone();
        let Some(removal_index) = entity_info
            .components
            .iter()
            .position(|ty| &ty.type_id == type_id)
        else {
            return None;
        };
        new_component_types.remove(removal_index);

        let mut entity_component_bundle = self
            .archetypes
            .get_mut(entity_info.archetype_ptr)
            .unwrap()
            .take_raw(entity_info.index);
        let removed_component = entity_component_bundle.remove(removal_index);

        let (archetype_ptr, new_archetype) = Self::get_or_create_archetype_static(
            &mut self.archetypes,
            &mut self.component_archetypes,
            new_component_types.clone(),
        );
        let archetype_index = new_archetype.insert_raw(entity, entity_component_bundle);

        entity_info.components = new_component_types;
        entity_info.archetype_ptr = archetype_ptr;
        entity_info.index = archetype_index;
        return Some(removed_component);
    }

    pub fn get_entity_components(&self, entity: Entity) -> Vec<TypeInfo> {
        let entity_info = self.entities.get(entity).unwrap();
        return entity_info.components.clone();
    }

    // Gets the minimum OBB of the entity's voxel model.
    pub fn get_entity_obb(
        &self,
        entity: Entity,
        voxel_registry: &VoxelModelRegistry,
    ) -> Option<OBB> {
        let mut query = self.query_one::<(&Transform, &RenderableVoxelEntity)>(entity);
        let Some((mut local_transform, mut renderable)) = query.get() else {
            return None;
        };
        if renderable.is_null() {
            return None;
        }

        let world_transform = self.get_world_transform(entity, &local_transform);
        let voxel_model = voxel_registry.get_dyn_model(renderable.voxel_model_id_unchecked());
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

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.has_value(entity)
    }

    pub fn duplicate(
        &mut self,
        entity: Entity,
        mut clone_ctx: GameComponentCloneContext<'_>,
    ) -> Entity {
        let new_entity_id = self.entities.next_free_handle();

        let entity_info = self
            .entities
            .get(entity)
            .expect("Tried to duplicate entity that does not exist.");
        assert!(entity_info.components.is_sorted());

        let entity_game_components = entity_info
            .components
            .iter()
            .filter_map(|type_info| {
                self.game_components
                    .contains_key(&type_info.type_id)
                    .then_some(type_info.clone())
            })
            .collect::<Vec<_>>();
        assert!(entity_game_components.is_sorted());

        let src_archetype = &mut self.archetypes[entity_info.archetype_ptr];
        let src_data = entity_game_components
            .iter()
            .map(|type_info| unsafe {
                (
                    type_info,
                    src_archetype
                        .get_raw(&type_info.type_id, entity_info.index)
                        .as_ptr(),
                )
            })
            .collect::<Vec<_>>();
        let cloned_data =
            src_data
                .iter()
                .map(|(type_info, src_data)| {
                    // Safety: We free the pointers after the data is copied to the new archetype.
                    let clone_dst_layout = type_info.layout(1);
                    let clone_dst = unsafe { std::alloc::alloc(clone_dst_layout) };
                    assert!(!clone_dst.is_null());

                    // Handle special case of duplicating by changing the name.
                    if type_info.type_id == std::any::TypeId::of::<GameEntity>() {
                        let game_entity_component = unsafe {
                            &*std::mem::transmute::<*const u8, *const GameEntity>(*src_data)
                        };
                        let new_game_entity = game_entity_component.duplicate();
                        unsafe { (clone_dst as *mut GameEntity).write(new_game_entity) };
                    } else {
                        let game_component_vtable = self
                            .game_components
                            .get(&type_info.type_id)
                            .unwrap()
                            .methods_vtable_ptr;
                        let game_component_ptr = unsafe {
                            std::mem::transmute::<
                                (*const u8, *const ()),
                                *const dyn GameComponentMethods,
                            >((*src_data, game_component_vtable))
                        };
                        let game_component = unsafe { game_component_ptr.as_ref().unwrap() };
                        game_component.clone_component(&mut clone_ctx, clone_dst);
                    }

                    (clone_dst, clone_dst_layout)
                })
                .collect::<Vec<_>>();

        // Since cloneable components may differ than total components for some reason.
        let (dst_archetype_ptr, dst_archetype) =
            self.get_or_create_archetype(entity_game_components.clone());
        let archetype_index = unsafe {
            dst_archetype.insert_raw(
                new_entity_id,
                cloned_data.iter().map(|(ptr, _)| *ptr).collect::<Vec<_>>(),
            )
        };

        for (cloned_ptr, cloned_dst_layout) in cloned_data {
            // Safety: We check it is not null, and it is allocated above.
            unsafe { std::alloc::dealloc(cloned_ptr, cloned_dst_layout) };
        }

        self.entities.push(EntityInfo {
            components: entity_game_components,
            archetype_ptr: dst_archetype_ptr,
            index: archetype_index,
        });

        return new_entity_id;
    }

    pub fn has_parent(&mut self, child: Entity, parent: Entity) -> bool {
        let parent_to_check = parent;
        let mut curr_parent = self.get::<&EntityParent>(child);
        while let Ok(parent) = curr_parent {
            let parent = parent.parent();
            if parent == parent_to_check {
                return true;
            }
            curr_parent = self.get::<&EntityParent>(parent);
        }

        return false;
    }

    // modify_transform should be true if you expect the child entity to stay in the same position
    // in world space, this will modify the child's transform to be positioned correctly relative to
    // the parent's transform.
    pub fn set_parent(&mut self, entity: Entity, parent: Option<Entity>, modify_transform: bool) {
        assert_ne!(
            Some(entity),
            parent,
            "Should not set entity parent as itself."
        );
        if let Some(new_parent) = parent {
            assert!(
                !self.has_parent(new_parent, entity),
                "Tried to set entity's parent to one of its children which is not allowed."
            );
            let old_world_transform = (modify_transform)
                .then(|| {
                    self.get::<&Transform>(entity)
                        .ok()
                        .map(|transform| self.get_world_transform(entity, &transform))
                })
                .flatten();
            let mut old_parent = self.get::<&mut EntityParent>(entity);
            if let Ok(ref mut parent_component) = old_parent {
                let last_parent = parent_component.parent();
                if last_parent == new_parent {
                    return;
                }
                parent_component.set_parent(new_parent);
                drop(old_parent);

                let mut old_parent_children = self
                    .get::<&mut EntityChildren>(last_parent)
                    .expect("Entity had this entity as a parent so it should have an EntityChildren component.");
                old_parent_children.children.remove(&entity);
                if old_parent_children.children.is_empty() {
                    drop(old_parent_children);
                    self.remove_one::<EntityChildren>(last_parent);
                } else {
                    drop(old_parent_children);
                }

                let new_parent_children = self.get::<&mut EntityChildren>(new_parent);
                if let Ok(mut new_parent_children) = new_parent_children {
                    new_parent_children.children.insert(entity);
                } else {
                    drop(new_parent_children);
                    let mut s = HashSet::new();
                    s.insert(entity);
                    self.insert_one(new_parent, EntityChildren { children: s });
                }
            } else {
                drop(old_parent);
                self.insert_one(entity, EntityParent::new(new_parent));

                let new_parent_children = self.get::<&mut EntityChildren>(new_parent);
                if let Ok(mut new_parent_children) = new_parent_children {
                    new_parent_children.children.insert(entity);
                } else {
                    drop(new_parent_children);
                    let mut s = HashSet::new();
                    s.insert(entity);
                    self.insert_one(new_parent, EntityChildren { children: s })
                        .expect("Failed to insert EntityChildren component.");
                }
            }

            if let Some(old_world_transform) = old_world_transform {
                let parent = self.get::<&EntityParent>(entity).unwrap().parent();
                let new_transform = if let Ok(parent_transform) = self.get::<&Transform>(parent) {
                    let parent_world_transform =
                        self.get_world_transform(parent, &parent_transform);
                    drop(parent_transform);
                    old_world_transform.as_relative_transform(&parent_world_transform)
                } else {
                    old_world_transform
                };
                *self.get::<&mut Transform>(entity).unwrap() = new_transform;
            }
        } else {
            let Ok(old_parent_ref) = self.get::<&EntityParent>(entity) else {
                return;
            };
            let old_parent = old_parent_ref.parent();
            if modify_transform && let Ok(mut child_transform) = self.get::<&mut Transform>(entity)
            {
                // Since we are removing the parent, just find the world transform and set the entity to that.
                let child_world_transform = self.get_world_transform(entity, &child_transform);
                *child_transform = child_world_transform;
            }

            let mut parent_children = self.get::<&mut EntityChildren>(old_parent).unwrap();
            parent_children.children.remove(&entity);
            let remove_children_component = parent_children.children.is_empty();
            drop(parent_children);
            drop(old_parent_ref);
            if remove_children_component {
                self.remove_one::<EntityChildren>(old_parent);
            }
            self.remove_one::<EntityParent>(entity);
        }
    }

    /// Despawns the given entity and removes the reference from its parent if it has one
    /// Does nothing if the entity does not exist.
    pub fn despawn(&mut self, entity: Entity, despawn_children: bool) {
        let entity_parent = self
            .get::<&EntityParent>(entity)
            .map(|p| p.parent().clone());
        if let Ok(entity_parent) = entity_parent {
            let mut parent_children = self.get::<&mut EntityChildren>(entity_parent)
                .expect("Despawned entity has a parent so that parent should have an `EntityChildren` component.");
            parent_children.children.remove(&entity);
            if parent_children.children.is_empty() {
                drop(parent_children);
                self.remove_one::<EntityChildren>(entity_parent);
            }
        }

        let entity_children = self
            .get::<&EntityChildren>(entity)
            .map(|c| c.children.clone());
        if let Ok(entity_children) = entity_children {
            for child in entity_children {
                if despawn_children {
                    self.despawn(child, true);
                } else {
                    self.try_remove_one::<EntityParent>(entity);
                }
            }
        }

        let Some(entity_info) = self.entities.get(entity) else {
            return;
        };

        assert!(entity_info.components.is_sorted());
        let archetype = &mut self.archetypes[entity_info.archetype_ptr];
        archetype.remove(entity_info.index);
        self.entities.remove(entity);
    }

    /// Converts the world transform to the proper local transform according to this entities parent.
    /// If this entity doesn't have a parent then local_transform == world_transform.
    pub fn get_world_to_local_transform(
        &self,
        entity: Entity,
        entity_world_transform: &Transform,
    ) -> Transform {
        let mut curr_transform = Transform::new();

        if let Ok(parent) = self.get::<&EntityParent>(entity)
            && let Ok(parent_transform) = self.get::<&Transform>(parent.parent())
        {
            let parent_world_transform =
                self.get_world_transform(parent.parent(), &parent_transform);
            return entity_world_transform.as_relative_transform(&parent_world_transform);
        }
        return entity_world_transform.clone();
    }

    pub fn get_world_transform(
        &self,
        entity: Entity,
        entity_local_transform: &Transform,
    ) -> Transform {
        let mut curr_transform = entity_local_transform.clone();

        let mut curr_parent = self.get::<&EntityParent>(entity);
        while let Ok(parent) = curr_parent {
            let Ok(parent_transform) = self.get::<&Transform>(parent.parent()) else {
                break;
            };
            curr_transform.apply_parent_transform(&parent_transform);
            curr_parent = self.get::<&EntityParent>(parent.parent());
        }

        return curr_transform;
    }

    /// Clones a game entity into a standalone struct containing and all of its components implementing the `GameComponent` trait.
    pub fn create_game_entity_asset(
        &self,
        entity_id: Entity,
        ctx: &mut GameComponentCloneContext<'_>,
    ) -> WorldGameEntityAsset {
        let entity_info = self.entities.get(entity_id).unwrap();
        debug_assert!(entity_info.components.is_sorted());
        let archetype = &self.archetypes[entity_info.archetype_ptr];

        let mut asset_game_entity = None;
        let mut asset_parent = None;
        let mut asset_children = Vec::new();
        let mut asset_components = HashMap::new();
        for type_info in &archetype.types {
            if type_info == &TypeInfo::new::<GameEntity>() {
                asset_game_entity = Some(
                    archetype
                        .get::<GameEntity>(type_info, entity_info.index)
                        .clone(),
                );
                continue;
            }
            if type_info == &TypeInfo::new::<EntityParent>() {
                let parent_id = archetype
                    .get::<EntityParent>(&TypeInfo::new::<EntityParent>(), entity_info.index)
                    .parent();
                asset_parent = Some(self.get_game_entity_uuid(parent_id));
                continue;
            }
            if type_info == &TypeInfo::new::<EntityChildren>() {
                let children = &archetype
                    .get::<EntityChildren>(&TypeInfo::new::<EntityChildren>(), entity_info.index)
                    .children;
                asset_children.extend(
                    children
                        .iter()
                        .map(|child_id| self.get_game_entity_uuid(*child_id)),
                );
                continue;
            }

            let Some(GameComponentType {
                methods_vtable_ptr, ..
            }) = self.game_components.get(&type_info.type_id)
            else {
                continue;
            };
            // Safety: We only use this pointer as a reference.
            let src_data = unsafe {
                archetype
                    .get_raw(&type_info.type_id, entity_info.index)
                    .as_ptr()
            };
            let (asset_component_data, dst_layout) =
                unsafe { Self::clone_component(src_data, type_info, *methods_vtable_ptr, ctx) };
            asset_components.insert(type_info.type_id, unsafe {
                WorldGameComponentAsset::new(type_info.clone(), asset_component_data)
            });
        }
        let game_entity =
            asset_game_entity.expect("Provided entity id doesn't have a GameEntity component.");

        WorldGameEntityAsset {
            name: game_entity.name,
            uuid: game_entity.uuid,
            parent: asset_parent,
            children: asset_children,
            components: asset_components,
        }
    }

    fn get_game_entity_uuid(&self, entity_id: Entity) -> Uuid {
        let entity_info = self.entities.get(entity_id).unwrap();
        debug_assert!(entity_info.components.is_sorted());
        let archetype = &self.archetypes[entity_info.archetype_ptr];
        let game_entity =
            archetype.get::<GameEntity>(&TypeInfo::new::<GameEntity>(), entity_info.index);
        game_entity.uuid.clone()
    }

    pub fn spawn_prefab(asset: &WorldGameEntityAsset) {}
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

pub struct ProjectSceneEntitiesVisitor<'a, 'b> {
    pub ctx: &'b mut ProjectSceneDeserializeContext<'a>,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut ProjectSceneEntitiesVisitor<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for &mut ProjectSceneEntitiesVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("entity array")
    }

    fn visit_seq<A>(mut self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut entity_visitor = EntityVisitor { ctx: self.ctx };
        loop {
            let Some(_) = seq.next_element_seed(&mut entity_visitor)? else {
                break;
            };
        }

        Ok(())
    }
}

pub struct ECSWorldSerializable<'a> {
    ecs_world: &'a ECSWorld,
    ctx: &'a GameComponentSerializeContext<'a>,
}

impl serde::Serialize for ECSWorldSerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = se.serialize_struct("Scene", 1)?;
        s.serialize_field("entities", &ECSWorldEntitiesSerializable { world: self })?;
        s.end()
    }
}

pub struct ECSWorldEntitiesSerializable<'a> {
    world: &'a ECSWorldSerializable<'a>,
}

impl serde::Serialize for ECSWorldEntitiesSerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut archetypes: HashSet<usize> = HashSet::new();
        let mut game_component_count = 0;
        for (name, ty) in self.world.ecs_world.game_component_names.iter() {
            game_component_count += 1;
            let Some(indices) = self.world.ecs_world.component_archetypes.get(ty) else {
                continue;
            };
            archetypes.extend(indices.iter().map(|i| *i));
        }

        let mut archetypes = Vec::from_iter(archetypes.into_iter());

        /// Borrow game component types for each archetype we will iterate over for the rest of the
        /// function.
        let mut borrows = Vec::new();
        for index in &archetypes {
            let archetype = &self.world.ecs_world.archetypes[*index];
            for ty in archetype.type_infos() {
                borrows.push(archetype.borrow_type(&ty.type_id));
            }
        }

        let mut seq = se.serialize_seq(None)?;
        for archetype_ptr in archetypes {
            let archetype = &self.world.ecs_world.archetypes[archetype_ptr];
            let has_game_entity = archetype
                .type_infos()
                .iter()
                .find(|type_info| type_info.type_id == std::any::TypeId::of::<GameEntity>())
                .is_some();
            if !has_game_entity {
                continue;
            }

            let game_component_indices = archetype
                .types
                .iter()
                .enumerate()
                .filter_map(|(i, type_info)| {
                    (self
                        .world
                        .ecs_world
                        .game_components
                        .contains_key(&type_info.type_id))
                    .then_some(i)
                })
                .collect::<Vec<_>>();
            for i in 0..archetype.len() {
                let Some(entity) = archetype.get_entity(i) else {
                    continue;
                };

                seq.serialize_element(&ECSWorldSceneEntitySerializable {
                    sup: self.world,
                    archetype,
                    archetype_index: i,
                    game_component_type_indices: &game_component_indices,
                    entity,
                })?;
            }
        }
        // We can now safely stop borrowing the archetype types we are using.
        for borrow in borrows {
            let b = borrow.get();
            borrow.set(b.unborrow());
        }
        seq.end()
    }
}

struct ECSWorldSceneEntitySerializable<'a> {
    sup: &'a ECSWorldSerializable<'a>,
    archetype: &'a ComponentArchetype,
    // Index within the archetype where this entity's component data is.
    archetype_index: usize,
    entity: Entity,
    game_component_type_indices: &'a [usize],
}

impl serde::Serialize for ECSWorldSceneEntitySerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = se.serialize_struct("Entity", 1)?;
        s.serialize_field(
            "components",
            &ECSWorldSceneEntityComponentsSerializable { sup: self },
        );
        s.end()
    }
}

struct ECSWorldSceneEntityComponentsSerializable<'a> {
    sup: &'a ECSWorldSceneEntitySerializable<'a>,
}

impl serde::Serialize for ECSWorldSceneEntityComponentsSerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = se.serialize_seq(Some(self.sup.game_component_type_indices.len()))?;

        for index in self.sup.game_component_type_indices {
            let type_info = &self.sup.archetype.type_infos()[*index];
            seq.serialize_element(&ECSWorldSceneEntityGameComponentSerializable {
                sup: self.sup,
                type_info,
            })?;
        }
        seq.end()
    }
}

struct ECSWorldSceneEntityGameComponentSerializable<'a> {
    sup: &'a ECSWorldSceneEntitySerializable<'a>,
    type_info: &'a TypeInfo,
}

impl serde::Serialize for ECSWorldSceneEntityGameComponentSerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = se.serialize_struct("GameComponent", 2)?;
        s.serialize_field(
            "name",
            &self
                .sup
                .sup
                .ecs_world
                .game_components
                .get(&self.type_info.type_id)
                .expect("Type should be a game component, must have filtered wrong.")
                .component_name,
        )?;
        s.serialize_field(
            "data",
            &ECSWorldSceneEntityGameComponentDataSerializable { sup: self },
        )?;
        s.end()
    }
}

struct ECSWorldSceneEntityGameComponentDataSerializable<'a> {
    sup: &'a ECSWorldSceneEntityGameComponentSerializable<'a>,
}

impl serde::Serialize for ECSWorldSceneEntityGameComponentDataSerializable<'_> {
    fn serialize<S>(&self, se: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let ecs_world = self.sup.sup.sup.ecs_world;
        let archetype = self.sup.sup.archetype;
        let ctx = self.sup.sup.sup.ctx;
        let src_data = unsafe {
            archetype
                .get_raw(&self.sup.type_info.type_id, self.sup.sup.archetype_index)
                .as_ptr()
        };
        let game_component = {
            let game_component_vtable = ecs_world
                .game_components
                .get(&self.sup.type_info.type_id)
                .unwrap()
                .methods_vtable_ptr;
            let game_component_ptr = unsafe {
                std::mem::transmute::<
                    (*const u8, GameComponentMethodsVtablePtr),
                    *const dyn GameComponentMethods,
                >((src_data, game_component_vtable))
            };

            unsafe { game_component_ptr.as_ref().unwrap() }
        };
        let mut erased_se = <dyn erased_serde::Serializer>::erase(se);
        // Ignore this result since the one we actually care about is stored in the serializer.
        let _ = game_component.serialize_component(ctx, &mut erased_se);
        erased_se.result()
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum EntityField {
    Components,
}

/// Visits the entity data and spawn the entity within the ctx ecs.
struct EntityVisitor<'a, 'b: 'a> {
    pub ctx: &'a mut ProjectSceneDeserializeContext<'b>,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut EntityVisitor<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: [&str; 1] = ["components"];
        de.deserialize_struct("Entity", &FIELDS, self)
    }
}

impl<'de> serde::de::Visitor<'de> for &mut EntityVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("entity Struct")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut components_visitor = EntityComponentsVisitor { ctx: self.ctx };

        let mut components_result = None;
        while let Some(key) = map.next_key::<EntityField>()? {
            match key {
                EntityField::Components => {
                    if components_result.is_some() {
                        return Err(serde::de::Error::duplicate_field("components"));
                    }
                    components_result = Some(map.next_value_seed(&mut components_visitor)?);
                }
            }
        }

        let Some(components_result) = components_result else {
            return Err(serde::de::Error::custom(
                "Scene does not contain an `components` field.",
            ));
        };

        let entity_id = self
            .ctx
            .ecs_world
            .spawn_raw(components_result.raw_component_data);
        if let Some(parent_uuid) = components_result.entity_parent {
            self.ctx.to_parent_entities.push((entity_id, parent_uuid));
        }

        Ok(())
    }
}

struct EntityComponentsVisitor<'a, 'b> {
    pub ctx: &'a mut ProjectSceneDeserializeContext<'b>,
}

struct EntityComponentsVisitorResult {
    raw_component_data: Vec<(TypeInfo, *const u8)>,
    entity_parent: Option<uuid::Uuid>,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut EntityComponentsVisitor<'_, '_> {
    type Value = EntityComponentsVisitorResult;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> serde::de::Visitor<'de> for &mut EntityComponentsVisitor<'_, '_> {
    type Value = EntityComponentsVisitorResult;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("expected Array with components")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut result = EntityComponentsVisitorResult {
            raw_component_data: Vec::new(),
            entity_parent: None,
        };
        let mut visitor = EntityComponentStructVisitor {
            ctx: self.ctx,
            result: &mut result,
        };
        while let Some(_) = seq.next_element_seed(&mut visitor)? {}
        Ok(result)
    }
}

struct EntityComponentStructVisitor<'a, 'b> {
    ctx: &'a mut ProjectSceneDeserializeContext<'b>,
    result: &'a mut EntityComponentsVisitorResult,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum EntityComponentStructField {
    Name,
    Data,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut EntityComponentStructVisitor<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: [&str; 2] = ["name", "data"];
        de.deserialize_struct("Component", &FIELDS, self)
    }
}

impl<'de> serde::de::Visitor<'de> for &mut EntityComponentStructVisitor<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("entity Struct")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut component_data_visitor = EntityComponentStructDataVisitor {
            ctx: self.ctx,
            game_component_de_method: None,
            dst_ptr: std::ptr::null_mut(),
        };

        let mut name = None;
        let mut data = None;
        while let Some(key) = map.next_key::<EntityComponentStructField>()? {
            match key {
                EntityComponentStructField::Name => {
                    if name.is_some() {
                        return Err(serde::de::Error::duplicate_field("name"));
                    }
                    name = Some(map.next_value::<String>()?);
                }
                EntityComponentStructField::Data => {
                    let Some(name) = &name else {
                        return Err(serde::de::Error::custom(
                            "Expect `name` to come before `data`.",
                        ));
                    };
                    if data.is_some() {
                        return Err(serde::de::Error::duplicate_field("data"));
                    }

                    let type_id = component_data_visitor.ctx.ecs_world.game_component_names.get(name)
                        .unwrap_or_else(|| panic!("Tried to deserialize component with GameComponent::NAME `{}` but there it is not registered in the ECSWorld, cant get type info.", name));
                    let game_component = component_data_visitor
                        .ctx
                        .ecs_world
                        .game_components
                        .get(&type_id)
                        .unwrap();
                    component_data_visitor.dst_ptr =
                        unsafe { std::alloc::alloc(game_component.type_info.layout(1)) };
                    if component_data_visitor.dst_ptr.is_null() {
                        panic!("Failed to allocate game component");
                    }
                    component_data_visitor.game_component_de_method =
                        Some(game_component.deserialize_fn);

                    map.next_value_seed(&mut component_data_visitor)?;
                    data = Some(component_data_visitor.dst_ptr);
                    // Make null again to catch any accidental second uses.
                    component_data_visitor.dst_ptr = std::ptr::null_mut();
                }
            }
        }

        let name = name.ok_or_else(|| serde::de::Error::missing_field("name"))?;
        let data = data.ok_or_else(|| serde::de::Error::missing_field("data"))?;

        let type_id = self.ctx.ecs_world.game_component_names.get(&name).ok_or_else(|| serde::de::Error::custom(format!("Provided component name `{}` doesn't map to any registered GameComponent type.", name)))?;
        let type_info = self
            .ctx
            .ecs_world
            .game_components
            .get(&type_id)
            .unwrap()
            .type_info
            .clone();
        self.result.raw_component_data.push((type_info, data));

        if *type_id == std::any::TypeId::of::<EntityParent>() {
            let component_ctx_parent = &mut self.ctx.component_ctx.entity_parent;
            if component_ctx_parent.is_nil() {
                return Err(serde::de::Error::custom(
                    "EntityParent component has a null parent uuid.",
                ));
            }
            self.result.entity_parent = Some(*component_ctx_parent);
            *component_ctx_parent = uuid::Uuid::nil();
        }

        Ok(())
    }
}

struct EntityComponentStructDataVisitor<'a, 'b> {
    ctx: &'a mut ProjectSceneDeserializeContext<'b>,
    game_component_de_method: Option<GameComponentDeserializeFnPtr>,
    dst_ptr: *mut u8,
}

impl<'de> serde::de::DeserializeSeed<'de> for &mut EntityComponentStructDataVisitor<'_, '_> {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut erased_de = <dyn erased_serde::Deserializer>::erase(de);
        unsafe {
            // GameComponent::deserialize_component(..)
            (self.game_component_de_method.unwrap())(
                self.ctx.component_ctx,
                &mut erased_de,
                self.dst_ptr,
            )
            .map_err(|err| serde::de::Error::custom(err))
        }?;
        Ok(())
    }
}
