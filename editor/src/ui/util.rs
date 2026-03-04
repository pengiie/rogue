/// Paints an arrow which is pointing right when collapsed or down when not.
pub fn paint_chevron_icon(ui: &mut egui::Ui, collapsed: bool, response: &egui::Response) {
    let visuals = ui.style().interact(response);

    let rect = response.rect;

    // Draw a pointy triangle arrow:
    let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), rect.height()));
    let rect = rect.expand(visuals.expansion);
    let mut points = vec![rect.left_top(), rect.right_top(), rect.center_bottom()];
    use std::f32::consts::TAU;
    // Rotate 90 degrees so point right when collapsed.
    let rotation = egui::emath::Rot2::from_angle(collapsed.then_some(-TAU / 4.0).unwrap_or(0.0));
    for p in &mut points {
        *p = rect.center() + rotation * (*p - rect.center());
    }

    ui.painter().add(egui::Shape::convex_polygon(
        points,
        visuals.fg_stroke.color,
        egui::Stroke::NONE,
    ));
}
