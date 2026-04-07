mod egui;
pub use egui::*;

pub mod egui_gpu;

pub mod util {
    use nalgebra::{UnitQuaternion, Vector3};

    pub fn position_ui(ui: &mut egui::Ui, position: &mut Vector3<f32>) {
        ui.horizontal(|ui| {
            ui.label("Position:");
            ui.label("X");
            ui.add(
                egui::DragValue::new(&mut position.x)
                    .suffix(" m")
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.label("Y");
            ui.add(
                egui::DragValue::new(&mut position.y)
                    .suffix(" m")
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.label("Z");
            ui.add(
                egui::DragValue::new(&mut position.z)
                    .suffix(" m")
                    .speed(0.01)
                    .fixed_decimals(2),
            );
        });
    }

    /// (pitch, yaw, roll)
    const DEGREE_DRAG_SPEED: f32 = 0.25;
    pub fn rotation_ui_euler(ui: &mut egui::Ui, rotation: &mut Vector3<f32>) {
        ui.horizontal(|ui| {
            ui.label("Rotation:");
            ui.label("X");
            // nalgebra uses positive rotation for clockwise but intuitively
            // counter-clockwise makes more sense since math.
            ui.add(
                egui::DragValue::new(&mut rotation.x)
                    .suffix("°")
                    .speed(DEGREE_DRAG_SPEED)
                    .fixed_decimals(2),
            );
            ui.label("Y");
            ui.add(
                egui::DragValue::new(&mut rotation.y)
                    .suffix("°")
                    .speed(DEGREE_DRAG_SPEED)
                    .fixed_decimals(2),
            );
            ui.label("Z");
            ui.add(
                egui::DragValue::new(&mut rotation.z)
                    .suffix("°")
                    .speed(DEGREE_DRAG_SPEED)
                    .fixed_decimals(2),
            );
        });
    }

    pub fn rotation_ui(ui: &mut egui::Ui, rotation: &mut UnitQuaternion<f32>) {
        ui.horizontal(|ui| {
            ui.label("Rotation:");
            ui.label("X");
            let (mut roll, mut pitch, mut yaw) = rotation.euler_angles();
            let original = Vector3::new(roll, pitch, yaw).map(|x| x.to_degrees());
            let mut edit = original.clone();
            // nalgebra uses positive rotation for clockwise but intuitively
            // counter-clockwise makes more sense since math.
            ui.add(
                egui::DragValue::new(&mut edit.x)
                    .suffix("°")
                    .speed(0.05)
                    .fixed_decimals(2),
            );
            ui.label("Y");
            ui.add(
                egui::DragValue::new(&mut edit.y)
                    .suffix("°")
                    .speed(0.05)
                    .fixed_decimals(2),
            );
            ui.label("Z");
            ui.add(
                egui::DragValue::new(&mut edit.z)
                    .suffix("°")
                    .speed(0.05)
                    .fixed_decimals(2),
            );
            let diff = edit - original;
            if diff.x != 0.0 {
                *rotation *=
                    UnitQuaternion::from_axis_angle(&Vector3::x_axis(), diff.x.to_radians());
            } else if diff.y != 0.0 {
                *rotation *=
                    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), diff.y.to_radians());
            } else if diff.z != 0.0 {
                *rotation *=
                    UnitQuaternion::from_axis_angle(&Vector3::z_axis(), diff.z.to_radians());
            }
        });
    }

    pub fn scale_ui(ui: &mut egui::Ui, scale: &mut Vector3<f32>) {
        ui.horizontal(|ui| {
            ui.label("Scale:");
            ui.label("X");
            ui.add(
                egui::DragValue::new(&mut scale.x)
                    .range(0.001..=1000.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.label("Y");
            ui.add(
                egui::DragValue::new(&mut scale.y)
                    .range(0.001..=1000.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
            ui.label("Z");
            ui.add(
                egui::DragValue::new(&mut scale.z)
                    .range(0.001..=1000.0)
                    .speed(0.01)
                    .fixed_decimals(2),
            );
        });
    }
}

// use std::{borrow::Borrow, str::FromStr};
//
// use gui::Egui;
// use nalgebra::{Vector2, Vector4};
// use rogue_macros::Resource;
//
// use super::{
//     asset::asset::Assets,
//     entity::{ecs_world::ECSWorld, scripting::Scripts},
//     graphics::renderer::Renderer,
//     input::Input,
//     resource::{Res, ResMut},
//     window::{
//         time::{Instant, Time},
//         window::Window,
//     },
// };
// use crate::{
//     common::color::Color,
//     engine::{event::Events, physics::physics_world::PhysicsWorld},
//     session::EditorSession,
//     settings::Settings,
// };
//
// pub mod egui;
//
// pub fn initialize_debug_ui_resource(app: &mut crate::app::App) {
//     let egui = Egui::new(&app.get_resource::<Window>());
//     app.insert_resource(egui);
//     app.insert_resource(UI::new());
// }
//
// #[derive(Resource)]
// pub struct UI {
//     /// Represented as [top, bottom, left, right].
//     pub content_padding: Vector4<f32>,
//
//     pub chunk_generator: ChunkGenerator,
// }
//
// pub struct DebugUIState {
//     pub zoom_factor: f32,
//     pub player_fov: f32,
//     pub fps: u32,
//     pub delta_time_ms: f32,
//     pub samples: u32,
//     pub polling_time_ms: u32,
//     pub draw_grid: bool,
//
//     pub generate_radius: u32,
//
//     pub brush_size: u32,
//     pub brush_color: Color,
//
//     pub last_ui_update: Instant,
// }
//
// impl Default for DebugUIState {
//     fn default() -> Self {
//         Self {
//             zoom_factor: 1.0,
//             player_fov: 90.0,
//             fps: 0,
//             samples: 0,
//             delta_time_ms: 0.0,
//             polling_time_ms: 250,
//             draw_grid: true,
//
//             generate_radius: 0,
//
//             brush_size: 1,
//             brush_color: Color::new_srgb(1.0, 0.2, 1.0),
//
//             last_ui_update: Instant::now(),
//         }
//     }
// }
//
// impl UI {
//     pub fn new() -> Self {
//         UI {
//             debug_state: DebugUIState::default(),
//             editor_state: EditorUIState::new(),
//             content_padding: Vector4::zeros(),
//             chunk_generator: ChunkGenerator::new(0),
//         }
//     }
//
//     pub fn content_offset(&self) -> Vector2<f32> {
//         self.content_padding.zx()
//     }
//
//     pub fn content_size(&self, window_size: Vector2<f32>) -> Vector2<f32> {
//         return window_size
//             - Vector2::new(
//                 self.content_padding.z + self.content_padding.w,
//                 self.content_padding.x + self.content_padding.y,
//             );
//     }
//
//     pub fn update(
//         mut egui: ResMut<Egui>,
//         mut ui: ResMut<UI>,
//         time: Res<Time>,
//         renderer: Res<Renderer>,
//         input: Res<Input>,
//     ) {
//         // Determine if we should poll for the current fps, ensures the fps doesn't change
//         // rapidly where it is unreadable.
//         if ui.debug_state.last_ui_update.elapsed().as_millis()
//             >= ui.debug_state.polling_time_ms.into()
//         {
//             ui.debug_state.last_ui_update = Instant::now();
//
//             ui.debug_state.fps = time.fps();
//             ui.debug_state.delta_time_ms = time.delta_time().as_micros() as f32 / 1000.0;
//         }
//     }
//
//     pub fn draw(
//         mut window: ResMut<Window>,
//         mut egui: ResMut<Egui>,
//         mut ui: ResMut<UI>,
//         mut voxel_world: ResMut<VoxelWorld>,
//         mut voxel_world_gpu: ResMut<VoxelWorldGpu>,
//         mut assets: ResMut<Assets>,
//         mut physics_world: ResMut<PhysicsWorld>,
//         mut settings: ResMut<Settings>,
//         mut ecs_world: ResMut<ECSWorld>,
//         mut editor: ResMut<Editor>,
//         mut session: ResMut<EditorSession>,
//         time: Res<Time>,
//         mut scripts: ResMut<Scripts>,
//         mut events: ResMut<Events>,
//         mut material_bank: ResMut<MaterialBank>,
//     ) {
//         let voxel_world: &mut VoxelWorld = &mut voxel_world;
//         let ui: &mut UI = &mut ui;
//         let debug_state = &mut ui.debug_state;
//         let editor_ui_state = &mut ui.editor_state;
//         let chunk_generator = &mut ui.chunk_generator;
//
//         let pixels_per_point = egui.pixels_per_point();
//         egui.resolve_ui(&mut window, |ctx, window| {
//             ui.content_padding = Vector4::zeros();
//             if editor.is_active {
//                 ui.content_padding = editor::ui::egui_editor_ui(
//                     ctx,
//                     &mut ecs_world,
//                     voxel_world,
//                     &mut voxel_world_gpu,
//                     &mut physics_world,
//                     &mut editor,
//                     editor_ui_state,
//                     &mut session,
//                     &mut assets,
//                     window,
//                     &time,
//                     &mut scripts,
//                     &mut settings,
//                     &mut events,
//                     &mut material_bank,
//                 );
//             } else {
//                 egui::Window::new("Debug")
//                     .current_pos(egui::pos2(4.0, 4.0))
//                     .movable(false)
//                     .show(ctx, |ui| {
//                         ui.set_width(150.0);
//
//                         ui.label(egui::RichText::new("Performance:").size(16.0));
//                         ui.label(format!("FPS: {}", debug_state.fps));
//                         ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
//                     });
//             }
//         });
//     }
// }
