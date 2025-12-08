use nalgebra::Vector3;

use crate::common::color::Color;
use crate::common::geometry::aabb::AABB;
use crate::engine::entity::component::{GameComponent, GameComponentSerializeContext};
use crate::engine::entity::ecs_world::Entity;
use crate::engine::physics::collider_registry::ColliderRegistry;
use crate::engine::voxel::voxel_world::VoxelWorld;
use crate::engine::{
    debug::DebugRenderer,
    physics::{collider_registry::ColliderId, transform::Transform},
};

pub struct ContactPoint {
    pub position: Vector3<f32>,
    // Distance along `ContactManifold.normal`, negative if penetrating.
    pub distance: f32,
    pub normal_impulse: f32,
    pub tangent_impulse: f32,
}

pub struct ContactManifold {
    pub points: Vec<ContactPoint>,
    pub normal: Vector3<f32>,
}

/// Output from the narrow phase step.
pub struct ContactPair {
    pub manifold: ContactManifold,
    pub entity_a: Entity,
    pub collider_a: ColliderId,
    pub entity_b: Entity,
    pub collider_b: ColliderId,
}

pub enum ColliderDebugColoring {
    Untouched,
    BroadPhaseCollision,
    NarrowPhaseCollision,
}

impl ColliderDebugColoring {
    pub fn color(&self) -> Color {
        match self {
            ColliderDebugColoring::Untouched => Color::new_srgb_hex("#FF00FF"),
            ColliderDebugColoring::BroadPhaseCollision => Color::new_srgb_hex("#B54342"),
            ColliderDebugColoring::NarrowPhaseCollision => Color::new_srgb_hex("#75EF22"),
        }
    }
}

pub type ColliderDeserializeFnPtr = unsafe fn(
    /*de: */ &mut dyn erased_serde::Deserializer,
    /*dst_ptr: */ *mut u8,
) -> erased_serde::Result<()>;

pub trait ColliderIntersectionTest<Marker> {
    fn run(
        &self,
        collider_id_a: &ColliderId,
        collider_id_b: &ColliderId,
        entity_transform_a: &Transform,
        entity_transform_b: &Transform,
        collider_registry: &ColliderRegistry,
    ) -> Option<ContactManifold>;
}

impl<F, A: Collider, B: Collider> ColliderIntersectionTest<(A, B)> for F
where
    F: Fn(&A, &B, &Transform, &Transform) -> Option<ContactManifold>,
{
    fn run(
        &self,
        collider_id_a: &ColliderId,
        collider_id_b: &ColliderId,
        entity_transform_a: &Transform,
        entity_transform_b: &Transform,
        collider_registry: &ColliderRegistry,
    ) -> Option<ContactManifold> {
        let collider_a = collider_registry.get_collider::<A>(collider_id_a);
        let collider_b = collider_registry.get_collider::<B>(collider_id_a);
        self(
            collider_a,
            collider_b,
            entity_transform_a,
            entity_transform_b,
        )
    }
}

type ColliderIntersectionTestErasedFn = fn(
    run_fn_ptr: *const (),
    collider_id_a: &ColliderId,
    collider_id_b: &ColliderId,
    entity_transform_a: &Transform,
    entity_transform_b: &Transform,
    collider_registry: &ColliderRegistry,
) -> Option<ContactManifold>;

pub struct ColliderIntersectionTestCaller {
    run_fn: ColliderIntersectionTestErasedFn,
    run_fn_ptr: *const (),
}

impl ColliderIntersectionTestCaller {
    pub fn new<F, Marker>(run_fn: F) -> Self
    where
        F: ColliderIntersectionTest<Marker> + 'static,
    {
        fn run_erased<F, Marker>(
            run_fn_ptr: *const (),
            collider_id_a: &ColliderId,
            collider_id_b: &ColliderId,
            entity_transform_a: &Transform,
            entity_transform_b: &Transform,
            collider_registry: &ColliderRegistry,
        ) -> Option<ContactManifold>
        where
            F: ColliderIntersectionTest<Marker> + 'static,
        {
            // Safety: i hope its safe :)
            let run_fn = unsafe { &*(run_fn_ptr as *const F) };
            run_fn.run(
                collider_id_a,
                collider_id_b,
                entity_transform_a,
                entity_transform_b,
                collider_registry,
            )
        };
        let run_fn_ptr = std::ptr::from_ref(&run_fn);
        Self {
            run_fn: run_erased::<F, Marker>,
            run_fn_ptr: run_fn_ptr as *const (),
        }
    }

    pub fn run_erased(
        &self,
        collider_id_a: &ColliderId,
        collider_id_b: &ColliderId,
        entity_transform_a: &Transform,
        entity_transform_b: &Transform,
        collider_registry: &ColliderRegistry,
    ) -> Option<ContactManifold> {
        (self.run_fn)(
            self.run_fn_ptr,
            collider_id_a,
            collider_id_b,
            entity_transform_a,
            entity_transform_b,
            collider_registry,
        )
    }
}

pub trait Collider: Clone + 'static {
    /// Name used for collider identification in collision tests and serialiation,
    /// must be unique between registered collider types.
    const NAME: &str;

    fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB;

    // Type erased serialization.
    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()>;
    unsafe fn deserialize_collider(
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()>;

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    ) {
    }
    fn collider_component_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "`Collider::collider_component_ui` has not been implemented for `{}`",
            std::any::type_name::<Self>()
        ));
    }
}

pub trait ColliderMethods: downcast::Any {
    fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB;

    // Type erased serialization.
    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()>;

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    );
    fn collider_component_ui(&mut self, ui: &mut egui::Ui);
}

impl<T: Collider> ColliderMethods for T {
    fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB {
        Collider::aabb(self, world_transform, voxel_world)
    }

    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        Collider::serialize_collider(self, ser)
    }

    fn render_debug(
        &self,
        world_transform: &Transform,
        debug_renderer: &mut DebugRenderer,
        coloring: ColliderDebugColoring,
    ) {
        Collider::render_debug(self, world_transform, debug_renderer, coloring);
    }

    fn collider_component_ui(&mut self, ui: &mut egui::Ui) {
        Collider::collider_component_ui(self, ui);
    }
}

downcast::downcast!(dyn ColliderMethods);
