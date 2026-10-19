//! Trading with villagers (M3.4): right-clicking one in survival opens the inventory screen at
//! it, its trades in the crafting list's place, as the server last sent them (`TradeOffers`):
//! each what the villager takes, an arrow and what it gives, and how many more times it'll make
//! it today, dimmed where what's carried won't pay or it's sold out. Clicking one asks the
//! server to make it (`Trade`), and the server's word that trading's over closes the screen. A
//! villager looked at is named under the crosshair.

use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use super::icons::{counted_icon, ItemIcons};
use super::look::{card, panel, shade, title, QUIET};
use super::Inventory;
use crate::client::player::LocalPlayer;
use crate::controls::{Action, Controls};
use crate::creatures::{Creature, CreatureKind, Villager};
use crate::game_mode::GameMode;
use crate::interaction::TargetCreature;
use crate::menu::{font, GameState, InWorld, TEXT_SHADOW};
use crate::protocol::{Trade, TradeOffers, UseCreature};
use crate::trading::{Offer, OFFERED};

// In pixels, as the crafting list's cards, but wider for the count left.
const CARD_WIDTH: f32 = 240.0;
const CARD_HEIGHT: f32 = 46.0;
// What's sold out says so in red.
const SOLD_OUT: Color = Color::srgb(0.95, 0.45, 0.4);

/// The villager the inventory screen's open at, if it's one, its name, and its trades as the
/// server last sent them, none until it's sent them. It all goes as the screen closes.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct TradeView {
    pub villager: Option<Entity>,
    pub name: String,
    pub offers: Vec<(Offer, u8)>,
}

#[derive(Component)]
pub(super) struct TradePanel;

#[derive(Component)]
pub(super) struct TradeName;

// A button making the trade at its place among those offered.
#[derive(Component)]
pub(super) struct TradeCard(pub(super) usize);

// The villager looked at's name, under the crosshair.
#[derive(Component)]
pub(super) struct NameLabel;

/// What a villager of `kind` is called: its name and its calling, "Edda the farmer".
pub fn called(villager: &Villager, kind: CreatureKind) -> String {
    format!("{} the {}", villager.name(), kind.name())
}

/// The panel of trades, hidden until the screen's opened at a villager, for the screen to put
/// beside its slots.
pub(super) fn spawn_trade(row: &mut ChildSpawnerCommands) {
    row.spawn((
        TradePanel,
        // shown by `show_trade` while the screen's open at a villager
        panel(),
    ))
    .with_children(|panel| {
        panel.spawn((TradeName, title("")));
        panel.spawn((
            Text::new("Trades today"),
            font(14.0),
            TextColor(QUIET),
            TEXT_SHADOW,
            Pickable::IGNORE,
        ));
        for offer in 0..OFFERED {
            panel
                .spawn((TradeCard(offer), card(CARD_WIDTH, CARD_HEIGHT)))
                .observe(trade);
        }
    });
}

// The server makes it, as it makes all changes to inventories (INV-4).
fn trade(
    press: On<PointerPress>,
    cards: Query<&TradeCard>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<TradeView>,
    mut trades: MessageWriter<Trade>,
) {
    let (Ok(card), PointerButton::Primary, Some(villager)) =
        (cards.get(press.entity), press.button, view.villager)
    else {
        return;
    };
    if view.offers.get(card.0).is_some_and(|&(_, left)| left > 0) {
        // shift-clicked, as many times as can be paid for
        trades.write(Trade {
            villager,
            offer: card.0 as u8,
            all: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        });
    }
}

/// Opens the screen at the villager right-clicked in survival, and asks the server to trade
/// with it, which sends its trades.
pub(super) fn talk(
    controls: Controls,
    target: Res<TargetCreature>,
    villagers: Query<(&Creature, &Villager)>,
    player: Query<&GameMode, With<LocalPlayer>>,
    mut view: ResMut<TradeView>,
    mut state: ResMut<NextState<GameState>>,
    mut uses: MessageWriter<UseCreature>,
) {
    let survival = player.single().is_ok_and(|mode| !mode.has_every_block());
    let Some(entity) = target
        .0
        .filter(|_| survival && controls.just_pressed(Action::Place))
    else {
        return;
    };
    if let Ok((creature, villager)) = villagers.get(entity) {
        uses.write(UseCreature { creature: entity });
        *view = TradeView {
            villager: Some(entity),
            name: called(villager, creature.0),
            offers: Vec::new(),
        };
        state.set(GameState::Inventory);
    }
}

/// Takes the trades the server sends for the villager the screen's open at, and closes the
/// screen once it says trading's over.
pub(super) fn follow_offers(
    mut sent: MessageReader<TradeOffers>,
    mut view: ResMut<TradeView>,
    mut state: ResMut<NextState<GameState>>,
) {
    for TradeOffers(offers) in sent.read() {
        if view.villager.is_none() {
            continue;
        }
        match offers {
            Some(offers) => view.offers = offers.clone(),
            None => state.set(GameState::Playing),
        }
    }
}

/// Shows the trades when the screen's open at a villager, redrawing the cards as the offers
/// change, dimmed where they can't be made.
#[allow(clippy::type_complexity)]
pub(super) fn show_trade(
    view: Res<TradeView>,
    icons: Res<ItemIcons>,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    mut panel: Query<&mut Node, With<TradePanel>>,
    mut name: Query<&mut Text, With<TradeName>>,
    mut cards: Query<(Entity, &TradeCard, &mut Node), Without<TradePanel>>,
    mut commands: Commands,
) {
    let display = match view.villager {
        Some(_) => Display::Flex,
        None => Display::None,
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
    let Ok(inventory) = player.single() else {
        return;
    };
    // redrawn as they're sent, and as what's carried changes, which a trade does
    if !view.is_changed() && !inventory.is_changed() {
        return;
    }
    for mut text in &mut name {
        text.0.clone_from(&view.name);
    }
    for (card, &TradeCard(index), mut node) in &mut cards {
        let Some(&(offer, left)) = view.offers.get(index) else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        let makeable = left > 0 && offer.affordable(&inventory);
        let (takes, gives) = (offer.takes, offer.gives);
        commands
            .entity(card)
            .despawn_related::<Children>()
            .with_children(|card| {
                card.spawn(counted_icon(&icons, takes.0, takes.1));
                card.spawn(arrow());
                card.spawn(counted_icon(&icons, gives.0, gives.1));
                let (stock, colour) = match left {
                    0 => ("Sold out".to_string(), SOLD_OUT),
                    left => (format!("{left} left"), QUIET),
                };
                card.spawn((
                    Text::new(stock),
                    font(14.0),
                    TextColor(colour),
                    TEXT_SHADOW,
                    Pickable::IGNORE,
                    Node {
                        margin: UiRect::left(Val::Auto),
                        ..default()
                    },
                ));
                if !makeable {
                    card.spawn(shade());
                }
            });
    }
}

// The arrow from what a trade takes to what it gives.
fn arrow() -> impl Bundle {
    (
        Text::new("→"),
        font(18.0),
        TextColor(QUIET),
        TEXT_SHADOW,
        Pickable::IGNORE,
        Node {
            margin: UiRect::horizontal(Val::Px(6.0)),
            ..default()
        },
    )
}

/// The label naming the villager looked at, under the crosshair.
pub(super) fn spawn_label(mut commands: Commands) {
    commands.spawn((
        InWorld,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            top: Val::Percent(53.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        Pickable::IGNORE,
        children![(
            NameLabel,
            Text::new(""),
            font(18.0),
            TextColor(Color::WHITE),
            TEXT_SHADOW,
            Visibility::Hidden,
        )],
    ));
}

/// Names the villager looked at, while one is.
pub(super) fn name_villager(
    target: Res<TargetCreature>,
    villagers: Query<(&Creature, &Villager)>,
    mut label: Query<(&mut Text, &mut Visibility), With<NameLabel>>,
) {
    let named = target.0.and_then(|entity| villagers.get(entity).ok());
    for (mut text, mut visibility) in &mut label {
        match named {
            Some((creature, villager)) => {
                let name = called(villager, creature.0);
                if text.0 != name {
                    text.0 = name;
                }
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
