use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use nalgebra::{UnitQuaternion, Vector3};
use rogue_macros::Resource;

use super::{
    capsule_collider::CapsuleCollider,
    plane_collider::PlaneCollider,
    rigid_body::{ForceType, RigidBody},
    transform::Transform,
};
use crate::{
    common::{
        color::Color,
        geometry::{aabb::AABB, shape::Shape},
    },
    engine::{
        debug::{DebugCapsule, DebugFlags, DebugLine},
        physics::{
            collider::{ColliderDebugColoring, ContactManifold, ContactPair},
            collider_component::EntityColliders,
            collider_registry::ColliderId,
            rigid_body::{self, RigidBodyType},
        },
        voxel::voxel_world::VoxelWorld,
    },
};
use crate::{
    common::{
        dyn_vec::{DynVecCloneable, TypeInfo},
        freelist::FreeList,
    },
    engine::{
        debug::DebugRenderer,
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            EntityChildren, EntityParent,
        },
        physics::collider_registry::ColliderRegistry,
        resource::{Res, ResMut},
        voxel::terrain::chunks::VoxelChunks,
        window::time::Instant,
    },
    session::EditorSession,
};

pub enum PhysicsTimestep {
    Max(Duration),
    Fixed(Duration),
}

pub struct PhysicsSettings {
    timestep: PhysicsTimestep,
    time_scale: f32,
    impulse_iterations: u32,
    baumgarte_iterations: u32,
    /// acceleration, meters / seconds^2
    gravity: Vector3<f32>,
}

impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            // 60 fps physics updates
            timestep: PhysicsTimestep::Fixed(Duration::from_secs_f32(1.0 / 60.0)),
            time_scale: 1.0,
            impulse_iterations: 5,
            baumgarte_iterations: 1,
            gravity: Vector3::new(0.0, -9.8, 0.0),
        }
    }
}

pub struct BroadPhase {
    collisions: Vec<[(Entity, ColliderId); 2]>,
    involved_colliders: HashSet<ColliderId>,
}

impl BroadPhase {
    pub fn new() -> Self {
        Self {
            collisions: Vec::new(),
            involved_colliders: HashSet::new(),
        }
    }

    pub fn reset(&mut self) {
        self.collisions.clear();
        self.involved_colliders.clear();
    }
}

pub struct NarrowPhase {
    contact_pairs: Vec<ContactPair>,
    collider_contacts: HashMap<ColliderId, Vec<u32>>,
}

impl NarrowPhase {
    pub fn new() -> Self {
        Self {
            contact_pairs: Vec::new(),
            collider_contacts: HashMap::new(),
        }
    }

    pub fn reset(&mut self) {
        self.contact_pairs.clear();
        self.collider_contacts.clear();
    }
}

#[derive(Resource)]
pub struct PhysicsWorld {
    // The timestep set by Self::next_time_step().
    curr_timestep: Duration,
    last_timestep: Instant,
    update_time: Instant,
    settings: PhysicsSettings,
    pub colliders: ColliderRegistry,
    // Whether to update rigid bodies or not.
    pub do_dynamics: bool,

    broad_phase: BroadPhase,
    narrow_phase: NarrowPhase,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            last_timestep: Instant::now(),
            update_time: Instant::now(),
            curr_timestep: Duration::ZERO,
            settings: PhysicsSettings::default(),
            colliders: ColliderRegistry::new(),
            do_dynamics: false,

            broad_phase: BroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
        }
    }

    pub fn reset_last_timestep(&mut self) {
        self.last_timestep = Instant::now();
    }

    // Determine the number of physics updates between the last physics update and now.
    pub fn physics_update_count(&mut self) -> u32 {
        self.update_time = Instant::now();
        let dur = self.update_time - self.last_timestep;
        match self.settings.timestep {
            PhysicsTimestep::Max(max_duration) => {
                (dur.as_secs_f32() / max_duration.as_secs_f32()).ceil() as u32
            }
            PhysicsTimestep::Fixed(fixed_duration) => {
                let updates = (dur.as_secs_f32() / fixed_duration.as_secs_f32()).floor() as u32;
                updates
            }
        }
    }

    pub fn render_debug_colliders(
        mut physics_world: ResMut<PhysicsWorld>,
        mut debug_renderer: ResMut<DebugRenderer>,
        editor: Res<Editor>,
        ecs_world: Res<ECSWorld>,
    ) {
        if !editor.show_collider_debug {
            return;
        }

        // Render colliders and color code them depending on their collision stage.
        for (entity, (transform, colliders)) in ecs_world
            .query::<(&Transform, &EntityColliders)>()
            .without::<(EntityParent,)>()
            .into_iter()
        {
            for collider_id in &colliders.colliders {
                let mut coloring = ColliderDebugColoring::Untouched;
                if physics_world
                    .broad_phase
                    .involved_colliders
                    .contains(collider_id)
                {
                    coloring = ColliderDebugColoring::BroadPhaseCollision;
                }
                if let Some(manifold_idx) = physics_world
                    .narrow_phase
                    .collider_contacts
                    .get(collider_id)
                {
                    coloring = ColliderDebugColoring::NarrowPhaseCollision;
                }

                let collider = physics_world.colliders.get_collider_dyn(collider_id);
                collider.render_debug(&transform, &mut debug_renderer, coloring);
            }
        }

        // Render all contact points from narrow phase.
        for narrow_contact_pair in &physics_world.narrow_phase.contact_pairs {
            log::debug!(
                "Contact between {:?} and {:?} with {:?} points",
                narrow_contact_pair.entity_a,
                narrow_contact_pair.entity_b,
                narrow_contact_pair.manifold.points.len()
            );
            for point in &narrow_contact_pair.manifold.points {
                debug_renderer.draw_capsule(DebugCapsule {
                    center: point.position,
                    orientation: UnitQuaternion::identity(),
                    radius: 0.2,
                    height: 0.0,
                    color: Color::new_srgb_hex("#DC3333"),
                    alpha: 0.75,
                    flags: DebugFlags::XRAY,
                });

                // Visualize each contact points penetration depth.
                let start = point.position;
                let end = start + narrow_contact_pair.manifold.normal * point.distance;
                debug_renderer.draw_line(DebugLine {
                    start,
                    end,
                    thickness: 0.1,
                    color: Color::new_srgb_hex("#2368DF"),
                    alpha: 0.75,
                    flags: DebugFlags::XRAY,
                });
            }
        }
    }

    pub fn validate_colliders_exist(&self, ecs_world: &mut ECSWorld) {
        for (entity, colliders) in ecs_world.query_mut::<&EntityColliders>().into_iter() {
            for collider_id in &colliders.colliders {
                assert!(self.colliders.contains_id(collider_id));
            }
        }
    }

    pub fn start_time_step(mut physics_world: ResMut<PhysicsWorld>) {
        physics_world.curr_timestep = match physics_world.settings.timestep {
            PhysicsTimestep::Max(duration) => {
                (physics_world.update_time - physics_world.last_timestep).min(duration)
            }
            PhysicsTimestep::Fixed(duration) => duration,
        };
    }

    pub fn time_step(&self) -> Duration {
        self.curr_timestep
    }

    pub fn end_time_step(mut physics_world: ResMut<PhysicsWorld>) {
        physics_world.last_timestep = physics_world.last_timestep + physics_world.curr_timestep;
        physics_world.curr_timestep = Duration::ZERO;
    }

    pub fn do_physics_update(
        mut physics_world: ResMut<PhysicsWorld>,
        mut ecs_world: ResMut<ECSWorld>,
        voxel_world: Res<VoxelWorld>,
    ) {
        let physics_world = &mut physics_world as &mut PhysicsWorld;
        let timestep = physics_world
            .curr_timestep
            .mul_f32(physics_world.settings.time_scale);

        if physics_world.do_dynamics {
            for (entity, (transform, rigid_body)) in ecs_world
                .query_mut::<(&mut Transform, &mut RigidBody)>()
                .without::<(EntityParent,)>()
                .into_iter()
            {
                if rigid_body.rigid_body_type == RigidBodyType::Dynamic {
                    // Apply gravity.
                    rigid_body.apply_force(
                        ForceType::Force,
                        rigid_body.mass() * physics_world.settings.gravity,
                    );

                    rigid_body.update(timestep, transform);
                }
            }
        }

        physics_world
            .colliders
            .update_entity_collider_positions(&mut ecs_world, &voxel_world);

        // Broad phase detection
        physics_world.broad_phase.reset();
        let mut tested_collision = HashSet::new();
        for (_, bin) in physics_world.colliders.bins.iter() {
            for (entity_a, collider_id_a) in bin {
                for (entity_b, collider_id_b) in bin {
                    if *entity_a == *entity_b {
                        continue;
                    }

                    if tested_collision.contains(&(*entity_a, *entity_b)) {
                        continue;
                    }
                    tested_collision.insert((*entity_a, *entity_b));
                    tested_collision.insert((*entity_b, *entity_a));

                    // TODO: Do collider triggers that don't require a rigid body.
                    let mut query = ecs_world
                        .query_many_mut::<(&mut Transform, &mut RigidBody), 2>([
                            *entity_a, *entity_b,
                        ]);
                    let [Some((transform_a, rigid_body_a)), Some((transform_b, rigid_body_b))] =
                        query.get()
                    else {
                        continue;
                    };

                    let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
                    let world_transform_b = ecs_world.get_world_transform(*entity_a, &transform_b);

                    let collider_a = physics_world.colliders.get_collider_dyn(collider_id_a);
                    let collider_b = physics_world.colliders.get_collider_dyn(collider_id_b);
                    let aabb_a = collider_a.aabb(&world_transform_a, &voxel_world);
                    let aabb_b = collider_b.aabb(&world_transform_b, &voxel_world);
                    let could_collide = aabb_a.intersects_aabb(&aabb_b);
                    if could_collide {
                        physics_world
                            .broad_phase
                            .collisions
                            .push([(*entity_a, *collider_id_a), (*entity_b, *collider_id_b)]);
                        physics_world
                            .broad_phase
                            .involved_colliders
                            .insert(*collider_id_a);
                        physics_world
                            .broad_phase
                            .involved_colliders
                            .insert(*collider_id_b);
                    }
                }
            }
        }

        // Narrow-phase contact point generation.
        physics_world.narrow_phase.reset();
        for [(entity_a, collider_a), (entity_b, collider_b)] in
            &physics_world.broad_phase.collisions
        {
            let mut query = ecs_world.query_many_mut::<&mut Transform, 2>([*entity_a, *entity_b]);
            let [Some(transform_a), Some(transform_b)] = query.get() else {
                continue;
            };

            let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
            let world_transform_b = ecs_world.get_world_transform(*entity_a, &transform_b);
            let Some(manifold) = physics_world.colliders.test_narrow_phase(
                collider_a,
                collider_b,
                &world_transform_a,
                &world_transform_b,
            ) else {
                continue;
            };

            let manifold_idx = physics_world.narrow_phase.contact_pairs.len();
            physics_world.narrow_phase.contact_pairs.push(ContactPair {
                manifold,
                entity_a: *entity_a,
                collider_a: *collider_a,
                entity_b: *entity_b,
                collider_b: *collider_b,
            });
            physics_world
                .narrow_phase
                .collider_contacts
                .entry(*collider_a)
                .or_default()
                .push(manifold_idx as u32);
            physics_world
                .narrow_phase
                .collider_contacts
                .entry(*collider_b)
                .or_default()
                .push(manifold_idx as u32);
        }

        if !physics_world.do_dynamics {
            return;
        }

        // Solve impulse contact constraints globally.
        for i in 0..physics_world.settings.impulse_iterations {
            for contact_pair in &mut physics_world.narrow_phase.contact_pairs {
                let (entity_a, entity_b, collider_a, collider_b) = (
                    &contact_pair.entity_a,
                    &contact_pair.entity_b,
                    &contact_pair.collider_a,
                    &contact_pair.collider_b,
                );
                let manifold = &mut contact_pair.manifold;

                let mut query = ecs_world
                    .query_many_mut::<(&mut Transform, &mut RigidBody), 2>([*entity_a, *entity_b]);
                let [Some((transform_a, rb_a)), Some((transform_b, rb_b))] = query.get() else {
                    continue;
                };

                let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
                let world_transform_b = ecs_world.get_world_transform(*entity_a, &transform_b);

                let friction_coeff = (rb_a.friction * rb_b.friction).sqrt();
                let normal = manifold.normal;
                for contact_point in &mut manifold.points {
                    let center_to_point_a = contact_point.position - world_transform_a.position;
                    let center_to_point_b = contact_point.position - world_transform_b.position;

                    let v_rel = rb_b.velocity() + rb_b.angular_linear_velocity(center_to_point_b)
                        - rb_a.velocity
                        - rb_a.angular_linear_velocity(center_to_point_a);
                    let v_rel_norm = v_rel.dot(&normal);

                    // Calculate normal impulse.
                    let eff_mass_rot_a = (rb_a.inv_inertia() * center_to_point_a.cross(&normal))
                        .cross(&center_to_point_a);
                    let eff_mass_rot_b = (rb_b.inv_inertia() * center_to_point_b.cross(&normal))
                        .cross(&center_to_point_b);
                    let k = rb_a.inv_mass()
                        + rb_b.inv_mass()
                        + (eff_mass_rot_a + eff_mass_rot_b).dot(&normal);
                    let mass_eff = 1.0 / k;

                    let restitution = rb_a.restitution.min(rb_b.restitution);
                    let vel_delta = (1.0 + restitution) * -v_rel_norm;
                    let delta_normal_impulse = vel_delta * mass_eff;

                    // Accumlate impulse for this point so we don't overall end up pulling.
                    let last_impulse = contact_point.normal_impulse;
                    contact_point.normal_impulse = (last_impulse + delta_normal_impulse).max(0.0);

                    let delta_normal_impulse = contact_point.normal_impulse - last_impulse;
                    let impulse_along_normal = delta_normal_impulse * normal;

                    if !rb_a.is_static() {
                        rb_a.velocity -= impulse_along_normal * rb_a.inv_mass();
                        rb_a.angular_velocity -=
                            rb_a.inv_inertia() * center_to_point_a.cross(&impulse_along_normal);
                    }
                    if !rb_b.is_static() {
                        rb_b.velocity += impulse_along_normal * rb_b.inv_mass();
                        rb_b.angular_velocity +=
                            rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_normal);
                    }

                    // Resulting v_rel after normal impulse applied.
                    let v_rel_post = (rb_b.velocity()
                        + rb_b.angular_linear_velocity(center_to_point_b)
                        - rb_a.velocity
                        - rb_a.angular_linear_velocity(center_to_point_a))
                    .dot(&normal);

                    // Calculate frictional impulse.
                    let v_rel = rb_b.velocity() + rb_b.angular_linear_velocity(center_to_point_b)
                        - rb_a.velocity
                        - rb_a.angular_linear_velocity(center_to_point_a);
                    let v_rel_norm = v_rel.dot(&normal);
                    let mut tangent = v_rel.cross(&normal).cross(&normal);
                    if tangent.norm_squared() > 0.0 {
                        tangent = tangent.normalize();
                    }
                    let v_rel_tangent = v_rel.dot(&tangent);

                    let eff_mass_rot_a = (rb_a.inv_inertia() * center_to_point_a.cross(&tangent))
                        .cross(&center_to_point_a);
                    let eff_mass_rot_b = (rb_b.inv_inertia() * center_to_point_b.cross(&tangent))
                        .cross(&center_to_point_b);
                    let k = rb_a.inv_mass()
                        + rb_b.inv_mass()
                        + (eff_mass_rot_a + eff_mass_rot_b).dot(&tangent);
                    let mass_eff = 1.0 / k;
                    let delta_friction_impulse = -v_rel_tangent * mass_eff;
                    // Contact point normal impulse ise always >= 0.
                    let max_friction_impulse = friction_coeff * contact_point.normal_impulse;
                    let last_friction_impulse = contact_point.tangent_impulse;
                    contact_point.tangent_impulse = (last_friction_impulse
                        + delta_friction_impulse)
                        .clamp(-max_friction_impulse, max_friction_impulse);
                    let delta_friction_impulse =
                        contact_point.tangent_impulse - last_friction_impulse;
                    let impulse_along_tangent = delta_friction_impulse * tangent;

                    if !rb_a.is_static() {
                        rb_a.velocity -= impulse_along_tangent * rb_a.inv_mass();
                        rb_a.angular_velocity -=
                            rb_a.inv_inertia() * center_to_point_a.cross(&impulse_along_tangent);
                    }
                    if !rb_b.is_static() {
                        rb_b.velocity += impulse_along_tangent * rb_b.inv_mass();
                        rb_b.angular_velocity +=
                            rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_tangent);
                    }

                    // Resulting v_rel after tangent impulse applied.
                    let v_rel_post = (rb_b.velocity()
                        + rb_b.angular_linear_velocity(center_to_point_b)
                        - rb_a.velocity
                        - rb_a.angular_linear_velocity(center_to_point_a))
                    .dot(&tangent);
                }
            }
        }

        // Positional overlap correction
        for i in 0..physics_world.settings.baumgarte_iterations {
            for contact_pair in &mut physics_world.narrow_phase.contact_pairs {
                let (entity_a, entity_b, collider_a, collider_b) = (
                    &contact_pair.entity_a,
                    &contact_pair.entity_b,
                    &contact_pair.collider_a,
                    &contact_pair.collider_b,
                );
                let manifold = &mut contact_pair.manifold;

                let mut query = ecs_world
                    .query_many_mut::<(&mut Transform, &mut RigidBody), 2>([*entity_a, *entity_b]);
                let [Some((transform_a, rb_a)), Some((transform_b, rb_b))] = query.get() else {
                    continue;
                };

                let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
                let world_transform_b = ecs_world.get_world_transform(*entity_a, &transform_b);

                let normal = manifold.normal;
                for contact_point in &mut manifold.points {
                    let center_to_point_a = contact_point.position - world_transform_a.position;
                    let center_to_point_b = contact_point.position - world_transform_b.position;

                    let steering_factor = 0.05;
                    let slop = 0.01;
                    let steering_velocity = contact_point.distance.signum()
                        * (steering_factor * (contact_point.distance.abs() - slop).max(0.0))
                            .min(0.2);

                    let eff_mass_rot_a = (rb_a.inv_inertia() * center_to_point_a.cross(&normal))
                        .cross(&center_to_point_a);
                    let eff_mass_rot_b = (rb_b.inv_inertia() * center_to_point_b.cross(&normal))
                        .cross(&center_to_point_b);
                    let k = rb_a.inv_mass() + rb_b.inv_mass();
                    //+ (eff_mass_rot_a + eff_mass_rot_b).dot(&normal);
                    let mass_eff = 1.0 / k;
                    let impulse = -steering_velocity * mass_eff;
                    let impulse_along_normal = impulse * normal;
                    // Apply psueduo impulse as positional correction.
                    if !rb_a.is_static() {
                        transform_a.position += -impulse_along_normal * rb_a.inv_mass();
                        transform_a.rotation *= UnitQuaternion::from_scaled_axis(
                            rb_a.inv_inertia() * -center_to_point_a.cross(&impulse_along_normal),
                        );
                    }

                    if !rb_b.is_static() {
                        transform_b.position += impulse_along_normal * rb_b.inv_mass();
                        transform_b.rotation *= UnitQuaternion::from_scaled_axis(
                            rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_normal),
                        );
                    }
                }
            }
        }
    }
}
