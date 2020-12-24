use bevy::{prelude::*, render::pass::ClearColor};
use geo_bevy::BuildBevyMeshes;

// System
fn layer_loaded(
    commands: &mut Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    layers: rgis_layers::ResLayers,
    events: Res<Events<rgis_layers::LayerLoaded>>,
    mut event_reader: Local<EventReader<rgis_layers::LayerLoaded>>,
    mut spawned_events: ResMut<Events<rgis_layers::LayerSpawned>>,
) {
    for event in event_reader.iter(&events) {
        let layers = layers.read().unwrap();
        let layer = match layers.get(event.0) {
            Some(l) => l,
            None => continue,
        };
        let material = materials.add(layer.color.into());

        let tl = time_logger::start(&format!("Triangulating and building {} mesh", layer.name));
        for mesh in layer
            .projected_geometry
            .geometry
            .build_bevy_meshes(geo_bevy::BuildBevyMeshesContext::new())
        {
            spawn_mesh(mesh, material.clone(), &mut meshes, commands);
        }
        tl.finish();

        spawned_events.send(rgis_layers::LayerSpawned(event.0));
    }
}

struct LayerSpawnedPlugin;

impl Plugin for LayerSpawnedPlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.add_system(layer_spawned.system());
    }
}

// System
fn layer_spawned(
    events: Res<Events<rgis_layers::LayerSpawned>>,
    layers: rgis_layers::ResLayers,
    mut camera_offset: ResMut<rgis_camera::CameraOffset>,
    mut camera_scale: ResMut<rgis_camera::CameraScale>,
    mut event_reader: Local<EventReader<rgis_layers::LayerSpawned>>,
) {
    for event in event_reader.iter(&events) {
        let layers = layers.read().unwrap();
        let layer = match layers.get(event.0) {
            Some(l) => l,
            None => continue,
        };
        let layer_center = layer.projected_bounding_rect.rect.center();
        // TODO: this scale math is inprecise. it should take into account
        // .     the height of the geometry. as well as the window size.
        let scale = layer.projected_bounding_rect.rect.width() / 1_000.;
        // TODO: only change the transform if there were no layers previously
        debug!("Moving camera to look at new layer");
        camera_offset.x = layer_center.x as f32;
        camera_offset.y = layer_center.y as f32;
        camera_scale.0 = scale as f32;
    }
}

pub fn spawn_mesh(
    mesh: Mesh,
    material: Handle<ColorMaterial>,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) {
    let sprite = SpriteBundle {
        material: material,
        mesh: meshes.add(mesh),
        sprite: Sprite {
            size: Vec2::new(1.0, 1.0),
            ..Default::default()
        },
        ..Default::default()
    };
    commands.spawn(sprite);
}

fn main() {
    let cli_values = rgis_cli::run();

    App::build()
        .add_resource(Msaa {
            samples: cli_values.msaa_sample_count,
        })
        .add_plugins(DefaultPlugins)
        .add_plugin(rgis_cli::Plugin(cli_values))
        .add_plugin(rgis_layers::RgisLayersPlugin)
        .add_plugin(rgis_file_loader::RgisFileLoaderPlugin)
        .add_system(layer_loaded.system())
        .add_plugin(rgis_mouse::Plugin)
        .add_plugin(rgis_keyboard::Plugin)
        .add_plugin(LayerSpawnedPlugin)
        .add_plugin(rgis_camera::RgisCamera)
        .add_plugin(rgis_ui::RgisUi)
        .add_resource(ClearColor(Color::WHITE))
        .run();
}
