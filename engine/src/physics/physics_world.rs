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
use crate::entity::{
    EntityChildren, EntityParent,
    ecs_world::{ECSWorld, Entity},
};
use crate::physics::collider_registry::ColliderRegistry;
use crate::physics::{
    collider::{ColliderDebugColoring, ContactManifold, ContactPair},
    collider_component::EntityColliders,
    collider_registry::ColliderId,
    rigid_body::{self, RigidBodyType},
};
use crate::resource::{Res, ResMut};
use crate::window::time::Instant;
use crate::{
    common::{
        color::Color,
        geometry::{aabb::AABB, ray::Ray, shape::Shape},
    },
    debug::debug_renderer::DebugRenderer,
};
use crate::{
    common::{
        dyn_vec::{DynVecCloneable, TypeInfo},
        freelist::FreeList,
    },
    physics::rigid_body::RigidBodyPositionInterpolation,
};

pub enum PhysicsTimestep {
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
            impulse_iterations: 15,
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
    last_timestep: Duration,
    last_update_instant: Instant,
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
            last_update_instant: Instant::now(),
            update_time: Instant::now(),
            curr_timestep: Duration::ZERO,
            last_timestep: Duration::ZERO,
            settings: PhysicsSettings::default(),
            colliders: ColliderRegistry::new(),
            do_dynamics: false,

            broad_phase: BroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
        }
    }

    pub fn reset_last_timestep(&mut self) {
        self.last_update_instant = Instant::now();
    }

    // Determine the number of physics updates between the last physics update and now.
    pub fn physics_update_count(&mut self) -> u32 {
        self.update_time = Instant::now();
        let dur = self.update_time - self.last_update_instant;
        match self.settings.timestep {
            PhysicsTimestep::Fixed(fixed_duration) => {
                let updates = (dur.as_secs_f32() / fixed_duration.as_secs_f32()).floor() as u32;
                updates
            }
        }
    }

    pub fn render_debug_colliders(
        mut physics_world: ResMut<PhysicsWorld>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
    ) {
        // Render colliders and color code them depending on their collision stage.
        for (entity, (transform, colliders)) in ecs_world
            .query::<(&Transform, &EntityColliders)>()
            .into_iter()
        {
            let world_transform = ecs_world.get_world_transform(entity, &transform);
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
                //collider.render_debug(&world_transform, &mut debug_renderer, color);
            }
        }

        // Render all contact points from narrow phase.
        for narrow_contact_pair in &physics_world.narrow_phase.contact_pairs {
            for point in &narrow_contact_pair.manifold.points {
                //debug_renderer.draw_capsule(DebugCapsule {
                //    center: point.position,
                //    orientation: UnitQuaternion::identity(),
                //    radius: 0.2,
                //    height: 0.0,
                //    color: Color::new_srgb_hex("#DC3333"),
                //    alpha: 0.75,
                //    flags: DebugFlags::XRAY,
                //});

                //// Visualize each contact points penetration depth.
                //let start = point.position;
                //let end = start + narrow_contact_pair.manifold.normal * point.distance;
                //debug_renderer.draw_line(DebugLine {
                //    start,
                //    end,
                //    thickness: 0.1,
                //    color: Color::new_srgb_hex("#2368DF"),
                //    alpha: 0.75,
                //    flags: DebugFlags::XRAY,
                //});
            }
        }

        // Render impulse lines.
        //for line in &physics_world.draw_impulse_lines {
        //    debug_renderer.draw_line(line.clone());
        //}
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
            PhysicsTimestep::Fixed(duration) => duration,
        };
    }

    pub fn time_step(&self) -> Duration {
        self.curr_timestep
    }

    pub fn last_time_step(&self) -> Duration {
        self.last_timestep
    }

    /// Different from time_step() since time_step() is only valid during a physics_update.
    pub fn time_since_last_physics_update(&self) -> Duration {
        Instant::now() - self.last_update_instant
    }

    pub fn end_time_step(mut physics_world: ResMut<PhysicsWorld>) {
        physics_world.last_update_instant =
            physics_world.last_update_instant + physics_world.curr_timestep;
        physics_world.last_timestep = physics_world.curr_timestep;
        physics_world.curr_timestep = Duration::ZERO;
    }

    pub fn ray_cast(&self, ray: Ray) -> Option<ColliderId> {
        None
    }

    /// Runs every frame.
    pub fn do_transform_interpolation(
        mut physics_world: ResMut<PhysicsWorld>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        if !physics_world.do_dynamics {
            return;
        }
        let timestep = physics_world.last_time_step();
        let last_timestep = physics_world.time_since_last_physics_update();
        let t = last_timestep.as_secs_f32() / timestep.as_secs_f32();
        for (entity, (transform, rigid_body)) in ecs_world
            .query_mut::<(&mut Transform, &mut RigidBody)>()
            .into_iter()
        {
            if matches!(
                rigid_body.rigid_body_type,
                RigidBodyType::Static | RigidBodyType::KinematicPositionBased
            ) {
                continue;
            }
            rigid_body.apply_to_transform(transform, t);
        }
    }

    pub fn sync_transforms(ecs_world: &mut ECSWorld) {
        for (entity, (transform, rigid_body)) in ecs_world
            .query_mut::<(&mut Transform, &mut RigidBody)>()
            .into_iter()
        {
            rigid_body.sync_transform(&transform);
        }
    }

    pub fn do_physics_update(
        mut physics_world: ResMut<PhysicsWorld>,
        mut ecs_world: ResMut<ECSWorld>,
    ) {
        let physics_world = &mut physics_world as &mut PhysicsWorld;
        let timestep = physics_world
            .curr_timestep
            .mul_f32(physics_world.settings.time_scale);

        if physics_world.do_dynamics {
            for (entity, (transform, rigid_body)) in ecs_world
                .query_mut::<(&mut Transform, &mut RigidBody)>()
                .into_iter()
            {
                if rigid_body.rigid_body_type == RigidBodyType::Static {
                    return;
                }

                // Initialize position/rotation of the rigid body if we haven't already.
                rigid_body.try_init_transform(transform);

                match rigid_body.rigid_body_type {
                    RigidBodyType::Static => {}
                    RigidBodyType::Kinematic => {
                        rigid_body.integrate_forces(timestep);
                    }
                    RigidBodyType::Dynamic => {
                        // Apply gravity.
                        rigid_body.apply_force(
                            ForceType::Force,
                            rigid_body.mass() * physics_world.settings.gravity,
                        );

                        rigid_body.integrate_forces(timestep);
                    }
                    RigidBodyType::KinematicPositionBased => {
                        rigid_body.derive_forces(timestep);
                    }
                }

                rigid_body.update_last_position_rotation();
            }
        }

        physics_world
            .colliders
            .update_entity_collider_positions(&mut ecs_world);

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

                    let mut query = ecs_world
                        .query_many_mut::<(&Transform, Option<&RigidBody>), 2>([
                            *entity_a, *entity_b,
                        ]);
                    let [Some((transform_a, rb_a)), Some((transform_b, rb_b))] = query.get() else {
                        log::debug!(
                            "Broad phase skipping collision between {:?} and {:?} due to missing transform or rigid body",
                            entity_a,
                            entity_b
                        );
                        continue;
                    };

                    // Skip collision checking between static bodies.
                    let is_static_a = rb_a.map(|rb| rb.is_static()).unwrap_or(true);
                    let is_static_b = rb_b.map(|rb| rb.is_static()).unwrap_or(true);
                    if is_static_a && is_static_b {
                        continue;
                    }

                    let world_transform_a = ecs_world.get_world_transform(*entity_a, transform_a);
                    let world_transform_b = ecs_world.get_world_transform(*entity_b, transform_b);

                    let collider_a = physics_world.colliders.get_collider_dyn(collider_id_a);
                    let collider_b = physics_world.colliders.get_collider_dyn(collider_id_b);
                    let voxel_collider_registry = &physics_world.colliders.voxel_collider_registry;
                    let aabb_a = collider_a
                        .aabb(&world_transform_a, voxel_collider_registry)
                        .expect("AABB should exist if collider was binned.");
                    let aabb_b = collider_b
                        .aabb(&world_transform_b, voxel_collider_registry)
                        .expect("AABB should exist if collider was binned.");
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
            let world_transform_b = ecs_world.get_world_transform(*entity_b, &transform_b);
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

        let mut static_body_a = RigidBody::new_static();
        let mut static_body_b = RigidBody::new_static();

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
                    .query_many_mut::<(&mut Transform, Option<&mut RigidBody>), 2>([
                        *entity_a, *entity_b,
                    ]);
                let [Some((transform_a, rb_a)), Some((transform_b, rb_b))] = query.get() else {
                    continue;
                };

                let rb_a = rb_a.unwrap_or(&mut static_body_a);
                let rb_b = rb_b.unwrap_or(&mut static_body_b);

                let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
                let world_transform_b = ecs_world.get_world_transform(*entity_b, &transform_b);

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

                    //physics_world.draw_impulse_lines.push(DebugLine {
                    //    start: contact_point.position,
                    //    end: contact_point.position + impulse_along_normal * 10.0,
                    //    thickness: 0.15,
                    //    color: Color::new_srgb_hex("#33DC57"),
                    //    alpha: 0.75,
                    //    flags: DebugFlags::XRAY,
                    //});

                    let angular_velocity_delta_a =
                        rb_a.inv_inertia() * -center_to_point_a.cross(&impulse_along_normal);
                    let angular_velocity_delta_b =
                        rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_normal);
                    //physics_world.draw_impulse_lines.push(DebugLine {
                    //    start: contact_point.position,
                    //    end: contact_point.position + angular_velocity_delta_a * 10.0,
                    //    thickness: 0.1,
                    //    color: Color::new_srgb_hex("#A357DC"),
                    //    alpha: 0.75,
                    //    flags: DebugFlags::XRAY,
                    //});
                    //physics_world.draw_impulse_lines.push(DebugLine {
                    //    start: contact_point.position,
                    //    end: contact_point.position + angular_velocity_delta_b * 10.0,
                    //    thickness: 0.1,
                    //    color: Color::new_srgb_hex("#A357DC"),
                    //    alpha: 0.75,
                    //    flags: DebugFlags::XRAY,
                    //});

                    rb_a.velocity -= impulse_along_normal * rb_a.inv_mass();
                    rb_a.set_angular_velocity(
                        rb_a.angular_velocity
                            + rb_a.inv_inertia() * -center_to_point_a.cross(&impulse_along_normal),
                    );
                    rb_b.velocity += impulse_along_normal * rb_b.inv_mass();
                    rb_b.set_angular_velocity(
                        rb_b.angular_velocity
                            + rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_normal),
                    );

                    //// Resulting v_rel after normal impulse applied.
                    //let v_rel_post = (rb_b.velocity()
                    //    + rb_b.angular_linear_velocity(center_to_point_b)
                    //    - rb_a.velocity
                    //    - rb_a.angular_linear_velocity(center_to_point_a))
                    //.dot(&normal);

                    //// Calculate frictional impulse.
                    let v_rel = rb_b.velocity() + rb_b.angular_linear_velocity(center_to_point_b)
                        - rb_a.velocity
                        - rb_a.angular_linear_velocity(center_to_point_a);
                    let v_rel_norm = v_rel.dot(&normal);
                    let mut tangent = v_rel - (v_rel.dot(&normal) * normal);
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

                    rb_a.velocity -= impulse_along_tangent * rb_a.inv_mass();
                    rb_a.set_angular_velocity(
                        rb_a.angular_velocity
                            + rb_a.inv_inertia() * -center_to_point_a.cross(&impulse_along_tangent),
                    );
                    rb_b.velocity += impulse_along_tangent * rb_b.inv_mass();
                    rb_b.set_angular_velocity(
                        rb_b.angular_velocity
                            + rb_b.inv_inertia() * center_to_point_b.cross(&impulse_along_tangent),
                    );

                    // Draw friction impulse line
                    //physics_world.draw_impulse_lines.push(DebugLine {
                    //    start: contact_point.position,
                    //    end: contact_point.position + impulse_along_tangent * 10000.0,
                    //    thickness: 0.15,
                    //    color: Color::new_srgb_hex("#FFAA00"),
                    //    alpha: 0.75,
                    //    flags: DebugFlags::XRAY,
                    //});

                    //// Resulting v_rel after tangent impulse applied.
                    //let v_rel_post = (rb_b.velocity()
                    //    + rb_b.angular_linear_velocity(center_to_point_b)
                    //    - rb_a.velocity
                    //    - rb_a.angular_linear_velocity(center_to_point_a))
                    //.dot(&normal);

                    //log::debug!(
                    //    "Iteration {}, {:?} v {:?}, relative_veloctiy {:?}",
                    //    i,
                    //    contact_pair.entity_a,
                    //    contact_pair.entity_b,
                    //    v_rel_post
                    //);
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
                    .query_many_mut::<(&mut Transform, Option<&mut RigidBody>), 2>([
                        *entity_a, *entity_b,
                    ]);
                let [Some((transform_a, rb_a)), Some((transform_b, rb_b))] = query.get() else {
                    continue;
                };

                let world_transform_a = ecs_world.get_world_transform(*entity_a, &transform_a);
                let world_transform_b = ecs_world.get_world_transform(*entity_b, &transform_b);
                let rb_a = rb_a.unwrap_or(&mut static_body_a);
                let rb_b = rb_b.unwrap_or(&mut static_body_b);

                let normal = manifold.normal;
                for contact_point in &mut manifold.points {
                    let center_to_point_a = contact_point.position - world_transform_a.position;
                    let center_to_point_b = contact_point.position - world_transform_b.position;

                    let steering_factor = 0.15;
                    let slop = 0.02;
                    let steering_velocity = contact_point.distance.signum()
                        * (steering_factor * (contact_point.distance.abs() - slop).max(0.0));

                    let eff_mass_rot_a = (rb_a.inv_inertia() * center_to_point_a.cross(&normal))
                        .cross(&center_to_point_a);
                    let eff_mass_rot_b = (rb_b.inv_inertia() * center_to_point_b.cross(&normal))
                        .cross(&center_to_point_b);
                    let k = rb_a.inv_mass()
                        + rb_b.inv_mass()
                        + (eff_mass_rot_a + eff_mass_rot_b).dot(&normal);
                    let mass_eff = 1.0 / k;
                    let impulse = -steering_velocity * mass_eff;
                    let impulse_along_normal = impulse * normal;
                    // Apply psueduo impulse as positional correction.
                    if !rb_a.is_static() {
                        transform_a.position +=
                            -impulse_along_normal * rb_a.inv_mass() * timestep.as_secs_f32();
                        transform_a.rotation *= UnitQuaternion::from_scaled_axis(
                            rb_a.inv_inertia()
                                * -center_to_point_a.cross(&impulse_along_normal)
                                * timestep.as_secs_f32(),
                        );
                        //physics_world.draw_impulse_lines.push(DebugLine {
                        //    start: world_transform_a.position,
                        //    end: contact_point.position,
                        //    thickness: 0.05,
                        //    color: Color::new_srgb_hex("#CCCCFF"),
                        //    alpha: 0.5,
                        //    flags: DebugFlags::XRAY,
                        //});
                        //physics_world.draw_impulse_lines.push(DebugLine {
                        //    start: contact_point.position,
                        //    end: contact_point.position - impulse_along_normal * 1000.0,
                        //    thickness: 0.15,
                        //    color: Color::new_srgb_hex("#EC1133"),
                        //    alpha: 0.75,
                        //    flags: DebugFlags::XRAY,
                        //});
                        //physics_world.draw_impulse_lines.push(DebugLine {
                        //    start: contact_point.position,
                        //    end: contact_point.position
                        //        + (rb_a.inv_inertia()
                        //            * -center_to_point_a.cross(&impulse_along_normal))
                        //            * 1000.0,
                        //    thickness: 0.1,
                        //    color: Color::new_srgb_hex("#C0FF00"),
                        //    alpha: 0.75,
                        //    flags: DebugFlags::XRAY,
                        //});
                    }

                    if !rb_b.is_static() {
                        transform_b.position +=
                            impulse_along_normal * rb_b.inv_mass() * timestep.as_secs_f32();
                        transform_b.rotation *= UnitQuaternion::from_scaled_axis(
                            rb_b.inv_inertia()
                                * center_to_point_b.cross(&impulse_along_normal)
                                * timestep.as_secs_f32(),
                        );
                        // Draw center to contact point line
                        // physics_world.draw_impulse_lines.push(DebugLine {
                        //     start: world_transform_b.position,
                        //     end: contact_point.position,
                        //     thickness: 0.05,
                        //     color: Color::new_srgb_hex("#CCCCFF"),
                        //     alpha: 0.5,
                        //     flags: DebugFlags::XRAY,
                        // });
                        // physics_world.draw_impulse_lines.push(DebugLine {
                        //     start: contact_point.position,
                        //     end: contact_point.position + impulse_along_normal * 1000.0,
                        //     thickness: 0.15,
                        //     color: Color::new_srgb_hex("#EC1133"),
                        //     alpha: 0.75,
                        //     flags: DebugFlags::XRAY,
                        // });
                        // physics_world.draw_impulse_lines.push(DebugLine {
                        //     start: contact_point.position,
                        //     end: contact_point.position
                        //         + (rb_b.inv_inertia()
                        //             * center_to_point_b.cross(&impulse_along_normal))
                        //             * 1000.0,
                        //     thickness: 0.1,
                        //     color: Color::new_srgb_hex("#C0FF00"),
                        //     alpha: 0.75,
                        //     flags: DebugFlags::XRAY,
                        // });
                    }
                }
            }
        }

        if physics_world.do_dynamics {
            for (entity, (transform, rigid_body)) in ecs_world
                .query_mut::<(&mut Transform, &mut RigidBody)>()
                .into_iter()
            {
                match rigid_body.rigid_body_type {
                    RigidBodyType::Static | RigidBodyType::KinematicPositionBased => {}
                    RigidBodyType::Kinematic => {
                        rigid_body.integrate_velocities(timestep);
                    }
                    RigidBodyType::Dynamic => {
                        rigid_body.integrate_velocities(timestep);
                    }
                }
            }
        }
    }
}
