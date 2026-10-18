//! How long blocks take to break in survival, and whether they drop anything (§5.1, §5.3): the
//! rules the client times the player's hold by, and the server checks it against.

use crate::block::BlockType;
use crate::items::Tool;

/// How many cracks a block shows on its way to breaking, one stage after another.
pub const CRACK_STAGES: usize = 8;

/// Whether breaking `block` with `held`, a tool or a bare hand, drops what it drops: if it needs
/// a tier, only with that kind of tool of that tier or better.
pub fn harvests(block: BlockType, held: Option<Tool>) -> bool {
    block.tier() == 0 || held.is_some_and(|tool| suits(block, tool))
}

// Whether `tool` is the kind for `block`, of the tier it needs.
fn suits(block: BlockType, tool: Tool) -> bool {
    Some(tool.kind) == block.tool() && tool.tier >= block.tier()
}

/// How long breaking `block` takes with `held`, in seconds: its hardness, divided by the tool's
/// speed if it's the tool for it and of the tier it needs, so a block without its tier takes as
/// long as it does by hand.
pub fn break_time(block: BlockType, held: Option<Tool>) -> f32 {
    let suited = held.filter(|&tool| suits(block, tool));
    block.hardness() / suited.map_or(1.0, |tool| tool.speed)
}

/// Which crack a block shows `held` seconds into its `time` to break.
pub fn crack_stage(held: f32, time: f32) -> usize {
    let done = if time > 0.0 { held / time } else { 1.0 };
    ((done * CRACK_STAGES as f32) as usize).min(CRACK_STAGES - 1)
}

#[cfg(test)]
mod tests {
    use super::{break_time, crack_stage, harvests, CRACK_STAGES};
    use crate::block::BlockType;
    use crate::items::Item;

    fn tool(item: Item) -> Option<crate::items::Tool> {
        item.tool()
    }

    #[test]
    fn test_breaking_takes_the_hardness_by_hand_divided_by_the_right_tools_speed() {
        assert_eq!(0.75, break_time(BlockType::Dirt, None));
        assert_eq!(7.5, break_time(BlockType::Stone, None));
        assert_eq!(3.0, break_time(BlockType::Log, None));
        // the tool for it is as many times quicker as its speed
        assert_eq!(
            7.5 / 4.0,
            break_time(BlockType::Stone, tool(Item::WoodenPickaxe))
        );
        assert_eq!(
            7.5 / 10.0,
            break_time(BlockType::Stone, tool(Item::IronPickaxe))
        );
        assert_eq!(3.0 / 6.0, break_time(BlockType::Log, tool(Item::StoneAxe)));
        assert_eq!(
            0.75 / 8.0,
            break_time(BlockType::Sand, tool(Item::CopperShovel))
        );
        // and another tool's no quicker than a hand
        assert_eq!(3.0, break_time(BlockType::Log, tool(Item::IronPickaxe)));
        assert_eq!(7.5, break_time(BlockType::Stone, tool(Item::IronShovel)));
        // plants break at a touch
        assert_eq!(0.0, break_time(BlockType::Poppy, tool(Item::IronAxe)));
    }

    #[test]
    fn test_without_the_tier_a_block_takes_as_long_as_by_hand_and_drops_nothing() {
        assert!(!harvests(BlockType::Stone, None));
        assert!(harvests(BlockType::Stone, tool(Item::WoodenPickaxe)));
        assert!(!harvests(BlockType::Stone, tool(Item::IronAxe)));
        assert!(harvests(BlockType::CoalOre, tool(Item::WoodenPickaxe)));
        assert!(!harvests(BlockType::CopperOre, tool(Item::WoodenPickaxe)));
        assert!(harvests(BlockType::CopperOre, tool(Item::StonePickaxe)));
        assert!(!harvests(BlockType::IronOre, tool(Item::StonePickaxe)));
        assert!(harvests(BlockType::IronOre, tool(Item::CopperPickaxe)));
        assert!(harvests(BlockType::IronOre, tool(Item::IronPickaxe)));
        assert_eq!(
            9.0,
            break_time(BlockType::IronOre, tool(Item::StonePickaxe))
        );
        // what needs no tier drops whatever breaks it
        for held in [None, tool(Item::WoodenShovel), tool(Item::IronPickaxe)] {
            assert!(harvests(BlockType::Dirt, held));
            assert!(harvests(BlockType::Log, held));
        }
    }

    #[test]
    fn test_every_tiered_block_has_a_tool_and_every_tool_a_block() {
        for block in BlockType::ALL {
            if block.tier() > 0 {
                assert!(block.tool().is_some(), "{block:?}");
            }
            assert!(block.hardness() >= 0.0, "{block:?}");
            // plants and torches go at a touch
            assert_eq!(
                block.needs_ground(),
                block.is_tangible() && block.hardness() == 0.0
            );
        }
        // and the best tier there is mines everything tiered
        for block in BlockType::ALL.into_iter().filter(|b| b.tier() > 0) {
            assert!(harvests(block, tool(Item::IronPickaxe)), "{block:?}");
        }
    }

    #[test]
    fn test_cracks_grow_through_their_stages_as_the_block_breaks() {
        assert_eq!(0, crack_stage(0.0, 2.0));
        assert_eq!(1, crack_stage(0.26, 2.0));
        assert_eq!(CRACK_STAGES / 2, crack_stage(1.0, 2.0));
        assert_eq!(CRACK_STAGES - 1, crack_stage(1.99, 2.0));
        assert_eq!(CRACK_STAGES - 1, crack_stage(5.0, 2.0));
        assert_eq!(CRACK_STAGES - 1, crack_stage(0.0, 0.0));
        let stages: Vec<_> = (0..20).map(|i| crack_stage(i as f32 * 0.1, 2.0)).collect();
        assert!(stages.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
