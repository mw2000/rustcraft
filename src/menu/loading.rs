//! The loading screen, while a world opens or a server's joined and the terrain round the player
//! loads, with a bar for how much of it has, rather than leaving them hanging in the air.

use bevy::prelude::*;

use super::widgets::{column, first, font, TEXT_SHADOW, WIDE};
use super::{back_button, GameState};
use crate::session::Loading;

const BAR_HEIGHT: f32 = 10.0;
const BAR_COLOR: Color = Color::srgb(0.5, 0.85, 0.4);

#[derive(Component)]
struct Doing;

#[derive(Component)]
struct Filled;

pub(super) fn build(app: &mut App) {
    app.add_systems(
        Update,
        show_progress
            .run_if(in_state(GameState::Loading))
            .run_if(resource_exists_and_changed::<Loading>),
    );
}

pub(super) fn spawn(In(root): In<Entity>, loading: Option<Res<Loading>>, mut commands: Commands) {
    let doing = loading
        .map(|loading| loading.doing.clone())
        .unwrap_or_default();
    commands.entity(root).with_child((
        column(),
        children![
            (
                Doing,
                Text::new(doing),
                font(20.0),
                TextColor(Color::WHITE),
                TEXT_SHADOW,
            ),
            (
                Node {
                    width: Val::Px(WIDE),
                    height: Val::Px(BAR_HEIGHT),
                    border: UiRect::all(Val::Px(2.0)),
                    margin: UiRect::vertical(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                BorderColor::all(Color::BLACK),
                children![(
                    Filled,
                    Node {
                        width: Val::Percent(0.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(BAR_COLOR),
                )],
            ),
            (back_button("Cancel", WIDE), first()),
        ],
    ));
}

fn show_progress(
    loading: Res<Loading>,
    mut doing: Query<&mut Text, With<Doing>>,
    mut filled: Query<(&mut Node, &mut Visibility), With<Filled>>,
) {
    for mut text in &mut doing {
        text.0.clone_from(&loading.doing);
    }
    for (mut node, mut visibility) in &mut filled {
        node.width = Val::Percent(loading.terrain.unwrap_or(0.0) * 100.0);
        *visibility = if loading.terrain.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
