use std::{
    any::TypeId,
    cell::Cell,
    collections::{HashMap, HashSet},
};

use rogue_macros::generate_tuples;

use crate::{
    common::dyn_vec::TypeInfo,
    engine::entity::{
        archetype::ComponentArchetype,
        component::{ComponentBorrowMap, ComponentRef, ComponentRefMut, ComponentTypeBorrow},
        ecs_world::{ECSWorld, Entity},
    },
};

pub trait Query {
    /// Result type from the query.
    type Item<'a>;

    fn collect_type_ids() -> Vec<TypeId>;
    fn collect_borrows(archetype: &ComponentArchetype) -> Vec<&Cell<ComponentTypeBorrow>>;
    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_>;
}

pub trait QueryItem {
    type Item<'a>;
    type Ref<'a>: QueryItemRef<'a>;

    fn item_type_id() -> TypeId;
    fn borrow(archetype: &ComponentArchetype) -> &Cell<ComponentTypeBorrow>;
    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_>;
}

pub trait QueryItemRef<'a>: Drop {
    fn create_ref(archetype: &'a ComponentArchetype, index: usize) -> Self
    where
        Self: Sized;
}

impl<T: 'static> QueryItem for &'_ T {
    type Item<'a> = &'a T;
    type Ref<'a> = ComponentRef<'a, T>;

    fn item_type_id() -> TypeId {
        std::any::TypeId::of::<T>()
    }

    fn borrow(archetype: &ComponentArchetype) -> &Cell<ComponentTypeBorrow> {
        archetype.borrow_type(&std::any::TypeId::of::<T>())
    }

    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_> {
        return archetype.get::<T>(&TypeInfo::new::<T>(), index);
    }
}

impl<T: 'static> QueryItem for &'_ mut T {
    type Item<'a> = &'a mut T;
    type Ref<'a> = ComponentRefMut<'a, T>;

    fn item_type_id() -> TypeId {
        std::any::TypeId::of::<T>()
    }

    fn borrow(archetype: &ComponentArchetype) -> &Cell<ComponentTypeBorrow> {
        archetype.borrow_type_mut(&std::any::TypeId::of::<T>())
    }

    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_> {
        // Safety: We dynamically borrow with `Self::borrow()` first, so any mutability should be safe.
        return unsafe {
            archetype
                .get_mut_unchecked::<T>(&TypeInfo::new::<T>(), index)
                .as_mut()
        };
    }
}

macro_rules! impl_query {
    ($($param:ident),+ , $($num:literal),*) => {
        impl<$($param: QueryItem),*> Query for ($($param,)*) {
            type Item<'a> = ($($param::Item<'a>),*);

            fn collect_type_ids() -> Vec<TypeId> {
                vec![
                    $(<$param as QueryItem>::item_type_id()),*
                ]
            }

            fn collect_borrows(archetype: &ComponentArchetype) -> Vec<&Cell<ComponentTypeBorrow>> {
                vec![
                    $($param::borrow(archetype)),*
                ]
            }

            fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_> {
                let items = ($($param::fetch(archetype, index)),*);
                return items;
            }
        }
    }
}

impl<T: QueryItem> Query for T {
    type Item<'a> = T::Item<'a>;

    fn collect_type_ids() -> Vec<TypeId> {
        vec![T::item_type_id()]
    }

    fn collect_borrows(archetype: &ComponentArchetype) -> Vec<&Cell<ComponentTypeBorrow>> {
        vec![T::borrow(archetype)]
    }

    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_> {
        return T::fetch(archetype, index);
    }
}

impl Query for () {
    type Item<'a> = ();

    fn collect_type_ids() -> Vec<TypeId> {
        vec![]
    }

    fn collect_borrows(_archetype: &ComponentArchetype) -> Vec<&Cell<ComponentTypeBorrow>> {
        vec![]
    }

    fn fetch(archetype: &ComponentArchetype, index: usize) -> Self::Item<'_> {
        return ();
    }
}

generate_tuples!(impl_query, 2, 16);

// Same as Bundle except it is not valid for the empty tuple type ().
pub trait ComponentMatchClause {
    fn component_type_ids() -> Vec<TypeId>;
}

macro_rules! impl_component_match_clause {
    ($($param:ident),+ , $($num:literal),*) => {
        impl<$($param: 'static),*> ComponentMatchClause for ($($param,)*) {
            fn component_type_ids() -> Vec<TypeId> {
                vec![
                    $(std::any::TypeId::of::<$param>()),*
                ]
            }
        }
    }
}

generate_tuples!(impl_component_match_clause, 1, 16);

pub struct QueryItemInfo {
    type_info: TypeInfo,
    is_mut: bool,
    // If the query is an optional query.
    is_optional: bool,
}

pub struct QueryBorrow<'a, Q: Query> {
    ecs_world: &'a ECSWorld,
    with: HashSet<TypeId>,
    without: HashSet<TypeId>,
    marker: std::marker::PhantomData<&'a Q>,
}

impl<'a, Q: Query> QueryBorrow<'a, Q> {
    pub fn new(ecs_world: &'a ECSWorld) -> Self {
        Self {
            ecs_world,
            with: Q::collect_type_ids().into_iter().collect::<HashSet<_>>(),
            without: HashSet::new(),
            marker: std::marker::PhantomData,
        }
    }

    pub fn with<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.with.insert(id);
            assert!(!self.without.contains(&id));
        }
        self
    }

    pub fn without<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.without.insert(id);
            assert!(!self.with.contains(&id));
        }
        self
    }

    pub fn execute(&mut self) {
        //'archetype_loop: for (types, archetype) in self.ecs_world.archetypes.iter() {
        //    let mut matching_types = 0;
        //    for ty in types {
        //        if self.without.contains(ty) {
        //            continue 'archetype_loop;
        //        }

        //        if self.with.contains(ty) {
        //            matching_types += 1;
        //        }
        //    }

        //    if matching_types != self.with.len() {
        //        continue 'archetype_loop;
        //    }
        //}
    }

    pub fn into_iter(self) -> QueryIter<'a, Q> {
        self.iter()
    }

    pub fn iter(&self) -> QueryIter<'a, Q> {
        let mut archetype_indices: HashMap<usize, usize> = HashMap::new();
        for ty in &self.with {
            let Some(indices) = self.ecs_world.component_archetypes.get(ty) else {
                continue;
            };
            'archetype_indices: for index in indices {
                let archetype = &self.ecs_world.archetypes[*index];
                for archetype_type_info in &archetype.types {
                    if self.without.contains(&archetype_type_info.type_id) {
                        continue 'archetype_indices;
                    }
                }
                let count = archetype_indices.entry(*index).or_insert(0);
                *count += 1;
            }
        }

        // Ensure all the archetype contains all queried types and sort by archetype index to keep
        // more queries consistent.
        let mut archetype_indices = archetype_indices
            .into_iter()
            .filter_map(|(index, count)| (count >= self.with.len()).then_some(index))
            .collect::<Vec<_>>();
        archetype_indices.sort();

        let mut borrows = Vec::new();
        for index in &archetype_indices {
            borrows.extend(Q::collect_borrows(&self.ecs_world.archetypes[*index]));
        }

        QueryIter::<'a, Q> {
            ecs_world: self.ecs_world,
            borrows,
            archetype_indices,
            curr_index: 0,
            marker: std::marker::PhantomData,
        }
    }
}

pub struct QueryIter<'a, Q: Query> {
    ecs_world: &'a ECSWorld,
    borrows: Vec<&'a Cell<ComponentTypeBorrow>>,
    archetype_indices: Vec<usize>,
    // Index within the archetype.
    curr_index: usize,
    marker: std::marker::PhantomData<&'a Q>,
}

impl<'a, Q: Query> Iterator for QueryIter<'a, Q> {
    type Item = (Entity, Q::Item<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        let Some(archetype_index) = self.archetype_indices.last() else {
            return None;
        };
        let archetype = &self.ecs_world.archetypes[*archetype_index];
        if self.curr_index >= archetype.len() {
            self.archetype_indices.pop();
            self.curr_index = 0;
            return self.next();
        }

        let entity = archetype.get_entity(self.curr_index);
        let query_item = Q::fetch(archetype, self.curr_index);
        self.curr_index += 1;
        return Some((entity, query_item));
    }
}

impl<'a, Q: Query> ExactSizeIterator for QueryIter<'a, Q> {}

impl<'a, Q: Query> Drop for QueryIter<'a, Q> {
    fn drop(&mut self) {
        for borrow in &self.borrows {
            let old = borrow.get();
            borrow.set(old.unborrow());
        }
    }
}

pub struct QueryOne<'a, Q: Query> {
    ecs_world: &'a ECSWorld,
    entity: Entity,
    borrows: Vec<&'a Cell<ComponentTypeBorrow>>,
    with: HashSet<TypeId>,
    without: HashSet<TypeId>,
    marker: std::marker::PhantomData<&'a Q>,
}

impl<'a, Q: Query> QueryOne<'a, Q> {
    pub fn new(ecs_world: &'a ECSWorld, entity: Entity) -> Self {
        Self {
            ecs_world,
            entity,
            with: Q::collect_type_ids().into_iter().collect::<HashSet<_>>(),
            without: HashSet::new(),
            borrows: Vec::new(),
            marker: std::marker::PhantomData,
        }
    }

    pub fn with<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.with.insert(id);
            assert!(!self.without.contains(&id));
        }
        self
    }

    pub fn without<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.without.insert(id);
            assert!(!self.with.contains(&id));
        }
        self
    }

    /// Must be called at most once. Returns None if the entity doesn't satisfy the query.
    pub fn get(&mut self) -> Option<Q::Item<'a>> {
        let archetype = self.ecs_world.find_archetype(self.entity);
        for type_id in &self.with {
            if !archetype.has_type_id(*type_id) {
                return None;
            }
        }
        for type_info in &archetype.types {
            if self.without.contains(&type_info.type_id) {
                return None;
            }
        }

        let entity_info = self.ecs_world.entities.get(self.entity).unwrap();
        let item = Q::fetch(archetype, entity_info.index);
        self.borrows = Q::collect_borrows(archetype);

        return Some(item);
    }
}

impl<Q: Query> Drop for QueryOne<'_, Q> {
    fn drop(&mut self) {
        for borrow in &self.borrows {
            let b = borrow.get();
            borrow.set(b.unborrow());
        }
    }
}

pub struct QueryMany<'a, Q: Query, const C: usize> {
    ecs_world: &'a ECSWorld,
    entities: [Entity; C],
    borrows: Vec<&'a Cell<ComponentTypeBorrow>>,
    with: HashSet<TypeId>,
    without: HashSet<TypeId>,
    marker: std::marker::PhantomData<&'a Q>,
}

impl<'a, Q: Query, const C: usize> QueryMany<'a, Q, C> {
    pub fn new(ecs_world: &'a ECSWorld, entities: [Entity; C]) -> Self {
        let mut set = HashSet::new();
        for entity in &entities {
            if set.contains(entity) {
                panic!("Each entity queried must be unique.");
            }
            set.insert(*entity);
        }

        Self {
            ecs_world,
            entities,
            with: Q::collect_type_ids().into_iter().collect::<HashSet<_>>(),
            without: HashSet::new(),
            borrows: Vec::new(),
            marker: std::marker::PhantomData,
        }
    }

    pub fn with<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.with.insert(id);
            assert!(!self.without.contains(&id));
        }
        self
    }

    pub fn without<W: ComponentMatchClause>(mut self) -> Self {
        for id in W::component_type_ids().into_iter() {
            self.without.insert(id);
            assert!(!self.with.contains(&id));
        }
        self
    }

    /// Must be called at most once. Returns None if the entity doesn't satisfy the query.
    pub fn get(&mut self) -> [Option<Q::Item<'a>>; C] {
        assert!(self.borrows.is_empty());
        let mut archetype_indices = HashSet::new();
        let mut items = [const { None }; C];
        'entity_loop: for (i, entity) in self.entities.iter().enumerate() {
            let Some(entity_info) = self.ecs_world.entities.get(*entity) else {
                continue;
            };
            archetype_indices.insert(entity_info.archetype_ptr);

            let archetype = &self.ecs_world.archetypes[entity_info.archetype_ptr];
            for type_id in &self.with {
                if !archetype.has_type_id(*type_id) {
                    continue 'entity_loop;
                }
            }
            for type_info in &archetype.types {
                if self.without.contains(&type_info.type_id) {
                    continue 'entity_loop;
                }
            }

            let item = Q::fetch(archetype, entity_info.index);
            items[i] = Some(item);
        }

        // Collect borrows
        for archetype_index in archetype_indices {
            let archetype = &self.ecs_world.archetypes[archetype_index];
            self.borrows.extend(Q::collect_borrows(archetype));
        }

        return items;
    }
}

impl<Q: Query, const C: usize> Drop for QueryMany<'_, Q, C> {
    fn drop(&mut self) {
        for borrow in &self.borrows {
            let b = borrow.get();
            borrow.set(b.unborrow());
        }
    }
}
