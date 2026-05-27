use std::f32::consts::TAU;
use std::sync::OnceLock;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2,
};
use serde_json::Value;

use crate::data::LoadedConfig;

#[derive(Default)]
pub struct MapInteraction {
    pub clicked: Option<String>,
}

pub struct MapCamera {
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for MapCamera {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
        }
    }
}

struct WorldMap {
    rings: Vec<Vec<[f64; 2]>>,
}

struct PopDraw {
    index: usize,
    base_pos: Pos2,
    pos: Pos2,
    overlap_count: usize,
}

pub fn draw_world_map(
    ui: &mut egui::Ui,
    config: Option<&LoadedConfig>,
    show_empty_pops: bool,
    highlighted_code: Option<&str>,
    camera: &mut MapCamera,
) -> MapInteraction {
    let available = ui.available_size();
    let size = Vec2::new(available.x.max(1.0), available.y.max(1.0));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let map_rect = rect;

    if response.dragged_by(egui::PointerButton::Primary)
        || response.dragged_by(egui::PointerButton::Secondary)
    {
        let delta = ui.input(|input| input.pointer.delta());
        camera.pan += delta;
        clamp_camera(camera, map_rect);
        ui.ctx().request_repaint();
    }

    if response.hovered() {
        let zoom_input = ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .filter(|pos| map_rect.contains(*pos))
                .map(|pos| {
                    let scroll = input.smooth_scroll_delta.y + input.raw_scroll_delta.y;
                    (pos, scroll)
                })
        });

        if let Some((cursor, scroll)) = zoom_input {
            if scroll.abs() > f32::EPSILON {
                zoom_at(camera, map_rect, cursor, scroll);
                ui.ctx().request_repaint();
            }
        }
    }

    painter.rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(12, 20, 27));
    painter.rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, Color32::from_rgb(63, 78, 88)),
        StrokeKind::Inside,
    );

    let map_painter = painter.with_clip_rect(map_rect);

    draw_graticule(&map_painter, map_rect, camera);
    draw_land(&map_painter, map_rect, camera);

    let mut interaction = MapInteraction::default();

    let Some(config) = config else {
        painter.text(
            map_rect.center(),
            Align2::CENTER_CENTER,
            "Loading current Valve SDR relay map...",
            FontId::proportional(18.0),
            Color32::WHITE,
        );
        return interaction;
    };

    let pointer = response.hover_pos();
    let pops = layout_visible_pops(config, show_empty_pops, map_rect, camera);

    for pop_draw in &pops {
        if pop_draw.overlap_count > 1 && map_rect.contains(pop_draw.pos) {
            map_painter.line_segment(
                [pop_draw.base_pos, pop_draw.pos],
                Stroke::new(0.8, Color32::from_rgba_unmultiplied(190, 205, 210, 100)),
            );
        }
    }

    let hovered_index = pointer.and_then(|pointer| {
        pops.iter()
            .filter_map(|pop_draw| {
                let pop = &config.pops[pop_draw.index];

                if pop.relays.is_empty() {
                    return None;
                }

                let dist = pointer.distance(pop_draw.pos);

                if dist < 12.0 {
                    Some((pop_draw.index, dist))
                } else {
                    None
                }
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    });

    for pop_draw in &pops {
        let pop = &config.pops[pop_draw.index];

        if !map_rect.contains(pop_draw.pos) {
            continue;
        }

        let hover = hovered_index == Some(pop_draw.index);
        let highlighted = highlighted_code == Some(pop.code.as_str()) || hover;

        let radius = if highlighted {
            7.0
        } else if pop_draw.overlap_count > 1 {
            5.5
        } else {
            5.0
        };

        let color = pop_color(pop.selected, !pop.relays.is_empty(), pop.tier);

        map_painter.circle_filled(pop_draw.pos, radius, color);
        map_painter.circle_stroke(
            pop_draw.pos,
            radius + 1.5,
            Stroke::new(1.0, Color32::from_black_alpha(190)),
        );

        if pop_draw.overlap_count > 1 {
            map_painter.circle_filled(
                pop_draw.base_pos,
                2.0,
                Color32::from_rgba_unmultiplied(220, 230, 235, 150),
            );
        }

        if highlighted {
            let label = format!(
                "{}\n{}\n{}\n{} relay IPs",
                pop.code.to_uppercase(),
                pop.desc,
                if pop.selected {
                    "Allowed"
                } else {
                    "Blocked when applied"
                },
                pop.relays.len()
            );

            draw_pop_label(
                &painter,
                pop_draw.pos + Vec2::new(10.0, -10.0),
                &label,
            );
        }
    }

    draw_map_legend(&painter, map_rect);

    if response.clicked() {
        if let Some(index) = hovered_index {
            let pop = &config.pops[index];

            if !pop.relays.is_empty() {
                interaction.clicked = Some(pop.code.clone());
            }
        }
    }

    interaction
}

fn pop_color(selected: bool, has_relays: bool, tier: u8) -> Color32 {
    if selected {
        Color32::from_rgb(78, 206, 118)
    } else if !has_relays {
        Color32::from_rgb(120, 130, 136)
    } else if tier == 0 {
        Color32::from_rgb(69, 160, 238)
    } else {
        Color32::from_rgb(235, 181, 82)
    }
}

fn draw_pop_label(painter: &egui::Painter, anchor: Pos2, label: &str) {
    let font_id = FontId::proportional(13.0);
    let line_height = 16.0;
    let horizontal_padding = 8.0;
    let vertical_padding = 6.0;
    let text_width = label
        .lines()
        .map(|line| line.chars().count() as f32 * 7.0)
        .fold(0.0, f32::max)
        .clamp(120.0, 340.0);
    let text_height = label.lines().count().max(1) as f32 * line_height;

    let background_rect = Rect::from_min_max(
        Pos2::new(
            anchor.x - horizontal_padding,
            anchor.y - text_height - vertical_padding,
        ),
        Pos2::new(
            anchor.x + text_width + horizontal_padding,
            anchor.y + vertical_padding,
        ),
    );

    painter.rect_filled(
        background_rect,
        CornerRadius::same(5),
        Color32::from_rgba_unmultiplied(5, 8, 11, 225),
    );
    painter.rect_stroke(
        background_rect,
        CornerRadius::same(5),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(180, 205, 220, 90)),
        StrokeKind::Inside,
    );

    painter.text(
        anchor,
        Align2::LEFT_BOTTOM,
        label,
        font_id,
        Color32::WHITE,
    );
}

fn draw_map_legend(painter: &egui::Painter, rect: Rect) {
    let legend_rect = Rect::from_min_size(
        rect.left_top() + Vec2::new(12.0, 12.0),
        Vec2::new(270.0, 116.0),
    );

    painter.rect_filled(
        legend_rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(12, 20, 27, 225),
    );
    painter.rect_stroke(
        legend_rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, Color32::from_rgb(63, 78, 88)),
        StrokeKind::Inside,
    );

    let title_pos = legend_rect.left_top() + Vec2::new(12.0, 10.0);
    painter.text(
        title_pos,
        Align2::LEFT_TOP,
        "Legend",
        FontId::proportional(13.0),
        Color32::WHITE,
    );

    let mut y = legend_rect.top() + 35.0;
    draw_legend_item(
        painter,
        Pos2::new(legend_rect.left() + 18.0, y),
        pop_color(true, true, 0),
        "Allowed - not blocked",
    );
    y += 20.0;
    draw_legend_item(
        painter,
        Pos2::new(legend_rect.left() + 18.0, y),
        pop_color(false, true, 0),
        "Blocked - Valve primary / tier 0",
    );
    y += 20.0;
    draw_legend_item(
        painter,
        Pos2::new(legend_rect.left() + 18.0, y),
        pop_color(false, true, 1),
        "Blocked - non-primary / partner",
    );
    y += 20.0;
    draw_legend_item(
        painter,
        Pos2::new(legend_rect.left() + 18.0, y),
        pop_color(false, false, 9),
        "No public relays - not selectable",
    );
}

fn draw_legend_item(painter: &egui::Painter, center: Pos2, color: Color32, label: &str) {
    painter.circle_filled(center, 5.0, color);
    painter.circle_stroke(
        center,
        6.5,
        Stroke::new(1.0, Color32::from_black_alpha(190)),
    );
    painter.text(
        center + Vec2::new(12.0, -7.0),
        Align2::LEFT_TOP,
        label,
        FontId::proportional(12.0),
        Color32::WHITE,
    );
}

fn layout_visible_pops(
    config: &LoadedConfig,
    show_empty_pops: bool,
    rect: Rect,
    camera: &MapCamera,
) -> Vec<PopDraw> {
    let mut pops = Vec::new();

    for (index, pop) in config.pops.iter().enumerate() {
        if pop.relays.is_empty() && !show_empty_pops {
            continue;
        }

        let base_pos = world_to_screen(rect, pop.lon, pop.lat, camera);

        if !rect.expand(32.0).contains(base_pos) {
            continue;
        }

        pops.push(PopDraw {
            index,
            base_pos,
            pos: base_pos,
            overlap_count: 1,
        });
    }

    let groups = overlap_groups(&pops);

    for group in groups {
        if group.len() <= 1 {
            continue;
        }

        for (slot, draw_index) in group.iter().enumerate() {
            let offset = overlap_offset(slot, group.len());
            pops[*draw_index].pos = pops[*draw_index].base_pos + offset;
            pops[*draw_index].overlap_count = group.len();
        }
    }

    pops
}

fn overlap_groups(pops: &[PopDraw]) -> Vec<Vec<usize>> {
    const OVERLAP_DISTANCE: f32 = 13.0;

    let mut visited = vec![false; pops.len()];
    let mut groups = Vec::new();

    for start in 0..pops.len() {
        if visited[start] {
            continue;
        }

        visited[start] = true;

        let mut group = Vec::new();
        let mut stack = vec![start];

        while let Some(current) = stack.pop() {
            group.push(current);

            for other in 0..pops.len() {
                if visited[other] {
                    continue;
                }

                if pops[current].base_pos.distance(pops[other].base_pos) <= OVERLAP_DISTANCE {
                    visited[other] = true;
                    stack.push(other);
                }
            }
        }

        groups.push(group);
    }

    groups
}

fn overlap_offset(slot: usize, count: usize) -> Vec2 {
    if count <= 1 {
        return Vec2::ZERO;
    }

    if count == 2 {
        return Vec2::new(if slot == 0 { -8.0 } else { 8.0 }, 0.0);
    }

    let radius = 12.0 + (count as f32).sqrt() * 3.0;
    let angle = slot as f32 / count as f32 * TAU - TAU * 0.25;

    Vec2::new(angle.cos() * radius, angle.sin() * radius)
}

fn draw_land(painter: &egui::Painter, rect: Rect, camera: &MapCamera) {
    let coast = Stroke::new(0.9, Color32::from_rgb(75, 113, 94));
    let border = Stroke::new(0.35, Color32::from_rgba_unmultiplied(129, 155, 138, 125));

    for ring in &world_map().rings {
        let mut segment: Vec<Pos2> = Vec::new();

        for window in ring.windows(2) {
            let [lon_a, lat_a] = window[0];
            let [lon_b, lat_b] = window[1];

            if (lon_b - lon_a).abs() > 180.0 {
                flush_land_segment(painter, &mut segment, coast, border);
                continue;
            }

            let a = world_to_screen(rect, lon_a, lat_a, camera);
            let b = world_to_screen(rect, lon_b, lat_b, camera);

            let max_reasonable_segment = rect.width().max(rect.height()) * 1.5;

            if a.distance(b) > max_reasonable_segment || !segment_might_be_visible(rect, a, b) {
                flush_land_segment(painter, &mut segment, coast, border);
                continue;
            }

            if segment.is_empty() {
                segment.push(a);
            }

            segment.push(b);
        }

        flush_land_segment(painter, &mut segment, coast, border);
    }
}

fn flush_land_segment(
    painter: &egui::Painter,
    segment: &mut Vec<Pos2>,
    coast: Stroke,
    border: Stroke,
) {
    if segment.len() >= 2 {
        painter.add(egui::Shape::line(segment.clone(), coast));
        painter.add(egui::Shape::line(std::mem::take(segment), border));
    } else {
        segment.clear();
    }
}

fn segment_might_be_visible(rect: Rect, a: Pos2, b: Pos2) -> bool {
    let padded = rect.expand(64.0);

    padded.contains(a) || padded.contains(b) || Rect::from_two_pos(a, b).intersects(padded)
}

fn draw_graticule(painter: &egui::Painter, rect: Rect, camera: &MapCamera) {
    let stroke = Stroke::new(0.55, Color32::from_rgba_unmultiplied(62, 84, 98, 135));

    for lon in (-180..=180).step_by(30) {
        let mut points = Vec::new();

        for lat in (-90..=90).step_by(5) {
            points.push(world_to_screen(rect, lon as f64, lat as f64, camera));
        }

        painter.add(egui::Shape::line(points, stroke));
    }

    for lat in (-60..=60).step_by(30) {
        let mut points = Vec::new();

        for lon in (-180..=180).step_by(5) {
            points.push(world_to_screen(rect, lon as f64, lat as f64, camera));
        }

        painter.add(egui::Shape::line(points, stroke));
    }
}

fn world_map() -> &'static WorldMap {
    static MAP: OnceLock<WorldMap> = OnceLock::new();
    MAP.get_or_init(load_world_map)
}

fn load_world_map() -> WorldMap {
    let json = include_str!("../assets/ne_110m_admin_0_countries.geojson");
    let value: Value = serde_json::from_str(json).expect("embedded world map is valid GeoJSON");
    let mut rings = Vec::new();

    if let Some(features) = value.get("features").and_then(Value::as_array) {
        for feature in features {
            let Some(geometry) = feature.get("geometry") else {
                continue;
            };

            let Some(kind) = geometry.get("type").and_then(Value::as_str) else {
                continue;
            };

            let Some(coords) = geometry.get("coordinates") else {
                continue;
            };

            match kind {
                "Polygon" => push_polygon(coords, &mut rings),
                "MultiPolygon" => {
                    if let Some(polygons) = coords.as_array() {
                        for polygon in polygons {
                            push_polygon(polygon, &mut rings);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    WorldMap { rings }
}

fn push_polygon(value: &Value, rings: &mut Vec<Vec<[f64; 2]>>) {
    let Some(polygon_rings) = value.as_array() else {
        return;
    };

    let Some(outer) = polygon_rings.first().and_then(Value::as_array) else {
        return;
    };

    let mut ring = Vec::with_capacity(outer.len());

    for point in outer {
        let Some(pair) = point.as_array() else {
            continue;
        };

        if pair.len() < 2 {
            continue;
        }

        let Some(lon) = pair[0].as_f64() else {
            continue;
        };

        let Some(lat) = pair[1].as_f64() else {
            continue;
        };

        ring.push([lon, lat]);
    }

    if ring.len() >= 3 {
        rings.push(ring);
    }
}

fn map_scale(rect: Rect, camera: &MapCamera) -> f32 {
    let base_scale = (rect.width() / 360.0).max(rect.height() / 180.0);
    base_scale * camera.zoom
}

fn world_to_screen(rect: Rect, lon: f64, lat: f64, camera: &MapCamera) -> Pos2 {
    let scale = map_scale(rect, camera);

    Pos2::new(
        rect.center().x + camera.pan.x + lon as f32 * scale,
        rect.center().y + camera.pan.y - lat as f32 * scale,
    )
}

fn screen_to_world(rect: Rect, pos: Pos2, camera: &MapCamera) -> [f64; 2] {
    let scale = map_scale(rect, camera);

    let lon = (pos.x - rect.center().x - camera.pan.x) / scale;
    let lat = -(pos.y - rect.center().y - camera.pan.y) / scale;

    [lon as f64, lat as f64]
}

fn zoom_at(camera: &mut MapCamera, rect: Rect, cursor: Pos2, scroll: f32) {
    let world_before = screen_to_world(rect, cursor, camera);

    camera.zoom = (camera.zoom * (scroll * 0.002).exp()).clamp(1.0, 8.0);

    let scale_after = map_scale(rect, camera);

    camera.pan = Vec2::new(
        cursor.x - rect.center().x - world_before[0] as f32 * scale_after,
        cursor.y - rect.center().y + world_before[1] as f32 * scale_after,
    );

    clamp_camera(camera, rect);
}

fn clamp_camera(camera: &mut MapCamera, rect: Rect) {
    camera.zoom = camera.zoom.clamp(1.0, 8.0);

    let scale = map_scale(rect, camera);

    let world_width = 360.0 * scale;
    let world_height = 180.0 * scale;

    let max_x = ((world_width - rect.width()) / 2.0).max(0.0);
    let max_y = ((world_height - rect.height()) / 2.0).max(0.0);

    camera.pan.x = camera.pan.x.clamp(-max_x, max_x);
    camera.pan.y = camera.pan.y.clamp(-max_y, max_y);
}