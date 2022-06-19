#![warn(
    clippy::unwrap_used,
    clippy::cast_lossless,
    clippy::unimplemented,
    clippy::expect_used
)]

use bevy::prelude::*;
use geo::bounding_rect::BoundingRect;
use geo::contains::Contains;
use std::{borrow, collections, sync};

#[derive(Clone, Debug)]
pub struct Layers {
    data: Vec<Layer>,
    // ID of the currently selected Layer
    pub selected_layer_id: Option<rgis_layer_id::LayerId>,
}

impl Default for Layers {
    fn default() -> Self {
        Self::new()
    }
}

impl Layers {
    pub fn new() -> Layers {
        Layers {
            data: vec![],
            selected_layer_id: None,
        }
    }

    #[inline]
    pub fn iter_bottom_to_top(&self) -> impl Iterator<Item = &Layer> {
        self.data.iter()
    }

    #[inline]
    pub fn iter_top_to_bottom(&self) -> impl Iterator<Item = &Layer> {
        self.data.iter().rev()
    }

    #[inline]
    pub fn count(&self) -> usize {
        self.data.len()
    }

    // coord is assumed to be projected
    pub fn containing_coord(&self, coord: geo::Coordinate<f64>) -> impl Iterator<Item = &Layer> {
        self.iter_top_to_bottom()
            .filter(move |layer| layer.projected_feature.contains(&coord))
    }

    // Returns whether the selected layer changed
    pub fn set_selected_layer_from_mouse_press(&mut self, coord: geo::Coordinate<f64>) -> bool {
        let selected_layer_id = {
            let mut iter = self.containing_coord(coord);
            let new_selected_layer = iter.next();
            if let Some(layer) = new_selected_layer {
                info!("A layer was clicked: {:?}", layer.name);
            }
            new_selected_layer.map(|layer| layer.id)
        };
        let prev_selected_layer_id = self.selected_layer_id;

        self.selected_layer_id = selected_layer_id;

        prev_selected_layer_id != self.selected_layer_id
    }

    fn get_index(&self, layer_id: rgis_layer_id::LayerId) -> Option<usize> {
        self.data.iter().position(|entry| entry.id == layer_id)
    }

    #[inline]
    pub fn get(&self, layer_id: rgis_layer_id::LayerId) -> Option<&Layer> {
        let index = self.get_index(layer_id)?;
        self.data.get(index)
    }

    #[inline]
    pub fn get_with_z_index(&self, layer_id: rgis_layer_id::LayerId) -> Option<(&Layer, usize)> {
        let index = self.get_index(layer_id)?;
        self.data.get(index).map(|layer| (layer, index))
    }

    #[inline]
    pub fn get_mut(&mut self, layer_id: rgis_layer_id::LayerId) -> Option<&mut Layer> {
        let index = self.get_index(layer_id)?;
        self.data.get_mut(index)
    }

    #[inline]
    pub fn remove(&mut self, layer_id: rgis_layer_id::LayerId) {
        if let Some(index) = self.get_index(layer_id) {
            self.data.remove(index);
        }
    }

    #[allow(unused)]
    pub fn selected_layer(&self) -> Option<&Layer> {
        self.selected_layer_id
            .and_then(|layer_id| self.get(layer_id))
    }

    fn next_layer_id(&self) -> rgis_layer_id::LayerId {
        rgis_layer_id::LayerId::new()
    }

    pub fn add(&mut self, unassigned_layer: UnassignedLayer) -> rgis_layer_id::LayerId {
        let layer_id = self.next_layer_id();
        let layer = Layer {
            unprojected_feature: unassigned_layer.unprojected_feature,
            projected_feature: unassigned_layer.projected_feature,
            color: unassigned_layer.color,
            name: unassigned_layer.name,
            visible: unassigned_layer.visible,
            id: layer_id,
            crs: unassigned_layer.crs,
        };
        self.data.push(layer);
        layer_id
    }
}

pub type Metadata = serde_json::Map<String, serde_json::Value>;

#[derive(Debug)]
pub struct UnassignedLayer {
    pub projected_feature: Feature,
    pub unprojected_feature: Feature,
    pub color: Color,
    pub metadata: Metadata,
    pub name: String,
    pub visible: bool,
    pub crs: String,
}

#[derive(thiserror::Error, Debug)]
pub enum LayerCreateError {
    #[error("Could not generate bounding box")]
    BoundingBox,
    #[cfg(target_arch = "wasm32")]
    #[error("{0}")]
    GeoProjJs(#[from] geo_proj_js::Error),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("{0}")]
    Proj(#[from] proj::TransformError),
}

impl UnassignedLayer {
    pub fn from_geometry(
        geometry: geo::Geometry<f64>,
        name: String,
        metadata: Option<Metadata>,
        source_crs: borrow::Cow<str>,
        target_crs: borrow::Cow<str>,
    ) -> Result<Self, LayerCreateError> {
        let unprojected_geometry = geometry;

        let mut projected_geometry = unprojected_geometry.clone();

        let tl = time_logger::start!("Reprojecting");
        #[cfg(target_arch = "wasm32")]
        {
            geo_proj_js::transform(&mut projected_geometry, &source_crs, &target_crs)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use geo::transform::Transform;
            projected_geometry.transform_crs_to_crs(&source_crs, &target_crs)?;
        }
        tl.finish();

        Ok(UnassignedLayer {
            unprojected_feature: Feature::from_geometry(unprojected_geometry)?,
            projected_feature: Feature::from_geometry(projected_geometry)?,
            color: colorous_color_to_bevy_color(next_colorous_color()),
            metadata: metadata.unwrap_or_else(serde_json::Map::new),
            crs: source_crs.into_owned(),
            name,
            visible: true,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Feature {
    pub geometry: geo::Geometry<f64>,
    pub properties: collections::HashMap<String, String>,
    pub bounding_rect: geo::Rect<f64>,
}

impl Contains<geo::Coordinate<f64>> for Feature {
    fn contains(&self, coord: &geo::Coordinate<f64>) -> bool {
        self.bounding_rect.contains(coord) && self.geometry.contains(coord)
    }
}

impl Feature {
    fn from_geometry(geometry: geo::Geometry<f64>) -> Result<Self, LayerCreateError> {
        let bounding_rect = geometry
            .bounding_rect()
            .ok_or(LayerCreateError::BoundingBox)?;

        Ok(Feature {
            geometry,
            properties: collections::HashMap::new(),
            bounding_rect,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    // {
    //    name: 'layer name',
    //    features: {
    //        <feature uuid> -> feature
    //     }
    // }
    // these should be vecs
    pub unprojected_feature: Feature,
    pub projected_feature: Feature,
    pub color: Color,
    pub id: rgis_layer_id::LayerId,
    pub name: String,
    pub visible: bool,
    pub crs: String,
}

fn colorous_color_to_bevy_color(colorous_color: colorous::Color) -> Color {
    Color::rgb_u8(colorous_color.r, colorous_color.g, colorous_color.b)
}

const COLORS: [colorous::Color; 10] = colorous::CATEGORY10;

fn next_colorous_color() -> colorous::Color {
    COLORS[next_color_index()]
}

fn next_color_index() -> usize {
    static COUNTER: sync::atomic::AtomicUsize = sync::atomic::AtomicUsize::new(0);
    COUNTER.fetch_add(1, sync::atomic::Ordering::Relaxed) % COLORS.len()
}

pub struct Plugin;

fn handle_toggle_layer_visibility_events(
    mut toggle_layer_visibility_event_reader: bevy::ecs::event::EventReader<
        rgis_events::ToggleLayerVisibilityEvent,
    >,
    mut layer_became_visible_event_writer: bevy::ecs::event::EventWriter<
        rgis_events::LayerBecameVisibleEvent,
    >,
    mut layer_became_hidden_event_writer: bevy::ecs::event::EventWriter<
        rgis_events::LayerBecameHiddenEvent,
    >,
    mut layers: ResMut<Layers>,
) {
    for event in toggle_layer_visibility_event_reader.iter() {
        let layer = match layers.get_mut(event.0) {
            Some(l) => l,
            None => {
                bevy::log::warn!("Could not find layer");
                continue;
            }
        };
        layer.visible = !layer.visible;
        if layer.visible {
            layer_became_visible_event_writer.send(rgis_events::LayerBecameVisibleEvent(event.0));
        } else {
            layer_became_hidden_event_writer.send(rgis_events::LayerBecameHiddenEvent(event.0));
        }
    }
}

fn handle_update_color_events(
    mut update_events: bevy::ecs::event::EventReader<rgis_events::UpdateLayerColorEvent>,
    mut updated_events: bevy::ecs::event::EventWriter<rgis_events::LayerColorUpdatedEvent>,
    mut layers: ResMut<Layers>,
) {
    for event in update_events.iter() {
        let layer = match layers.get_mut(event.0) {
            Some(l) => l,
            None => {
                bevy::log::warn!("Could not find layer");
                continue;
            }
        };
        layer.color = event.1;
        updated_events.send(rgis_events::LayerColorUpdatedEvent(event.0));
    }
}

fn handle_delete_layer_events(
    mut delete_layer_event_reader: bevy::ecs::event::EventReader<rgis_events::DeleteLayerEvent>,
    mut layer_deleted_event_writer: bevy::ecs::event::EventWriter<rgis_events::LayerDeletedEvent>,
    mut layers: ResMut<Layers>,
) {
    for event in delete_layer_event_reader.iter() {
        layers.remove(event.0);
        layer_deleted_event_writer.send(rgis_events::LayerDeletedEvent(event.0));
    }
}

fn handle_move_layer_events(
    mut move_layer_event_reader: bevy::ecs::event::EventReader<rgis_events::MoveLayerEvent>,
    mut layer_z_index_updated_event_writer: bevy::ecs::event::EventWriter<
        rgis_events::LayerZIndexUpdatedEvent,
    >,
    mut layers: ResMut<Layers>,
) {
    for event in move_layer_event_reader.iter() {
        let (_, old_z_index) = match layers.get_with_z_index(event.0) {
            Some(result) => result,
            None => {
                bevy::log::warn!("Could not find layer");
                continue;
            }
        };

        let new_z_index = match event.1 {
            rgis_events::MoveDirection::Up => old_z_index + 1,
            rgis_events::MoveDirection::Down => old_z_index - 1,
        };

        let other_layer_id = match layers.data.get(new_z_index) {
            Some(layer) => layer.id,
            None => {
                bevy::log::warn!("Could not find layer");
                continue;
            }
        };

        layers.data.swap(old_z_index, new_z_index);

        layer_z_index_updated_event_writer.send(rgis_events::LayerZIndexUpdatedEvent(event.0));
        layer_z_index_updated_event_writer
            .send(rgis_events::LayerZIndexUpdatedEvent(other_layer_id));
    }
}

fn handle_map_clicked_events(
    mut map_clicked_event_reader: bevy::ecs::event::EventReader<rgis_events::MapClickedEvent>,
    mut layers: ResMut<Layers>,
) {
    for event in map_clicked_event_reader.iter() {
        layers.set_selected_layer_from_mouse_press(event.0);
    }
}

impl bevy::app::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Layers::new())
            .add_system(handle_toggle_layer_visibility_events)
            .add_system(handle_update_color_events)
            .add_system(handle_move_layer_events)
            .add_system(handle_delete_layer_events)
            .add_system(handle_map_clicked_events);
    }
}
