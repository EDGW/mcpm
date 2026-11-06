//! Gamerule assistance gated by an explicit enumeration in the server's help response.

use super::{Diagnostic, Query};

pub(super) struct Gamerule {
    types: Vec<String>,
}

impl Gamerule {
    pub fn parse(arguments: &str) -> Option<Self> {
        let (choices, suffix) = arguments.trim().strip_prefix('(')?.split_once(')')?;
        if !suffix.is_empty()
            && (!suffix.starts_with(char::is_whitespace) || suffix.trim() != "<value>")
        {
            return None;
        }
        let mut types = Vec::new();
        let mut alternatives = 0;
        for choice in choices.split('|').map(str::trim) {
            if choice.is_empty()
                || choice
                    .chars()
                    .any(|ch| ch.is_whitespace() || ch.is_control() || "()<>[]|".contains(ch))
            {
                return None;
            }
            alternatives += 1;
            if !types.iter().any(|name| name == choice) {
                types.push(choice.to_owned());
            }
        }
        (alternatives >= 2).then_some(Self { types })
    }

    pub fn query<'a, 'input>(&'a self, arguments: &'input str) -> Query<'a, 'input> {
        let end = arguments
            .find(char::is_whitespace)
            .unwrap_or(arguments.len());
        let name = &arguments[..end];
        let complete = end < arguments.len();
        let known = !name.is_empty() && self.types.iter().any(|candidate| candidate == name);
        let mut query = Query {
            candidates: if known && complete { &[] } else { &self.types },
            prefix: name,
            replacement: 0..end,
            diagnostic: None,
            validates_value: |name| ValueType::for_rule(name).is_some(),
        };
        if name.is_empty() {
            return query;
        }
        if !known {
            if complete
                || !self
                    .types
                    .iter()
                    .any(|candidate| candidate.starts_with(name))
            {
                query.diagnostic = Some(Diagnostic {
                    range: 0..end,
                    message: "Unknown gamerule type",
                });
            }
            return query;
        }
        let tail = &arguments[end..];
        let value = tail.trim();
        if value.is_empty() {
            return query;
        }
        if let Some(kind) = ValueType::for_rule(name) {
            let valid = match kind {
                ValueType::Boolean => matches!(value, "true" | "false"),
                ValueType::Integer => value.parse::<i64>().is_ok(),
            };
            if !valid {
                let start = arguments.len() - tail.trim_start().len();
                query.diagnostic = Some(Diagnostic {
                    range: start..start + value.len(),
                    message: match kind {
                        ValueType::Boolean => "Expected true or false",
                        ValueType::Integer => "Expected a signed 64-bit integer",
                    },
                });
            }
        }
        query
    }
}

/// Only the supplied value types are known locally; defaults and server-specific bounds are not.
enum ValueType {
    Boolean,
    Integer,
}

impl ValueType {
    fn for_rule(name: &str) -> Option<Self> {
        match name {
            "fire_spread_radius_around_player"
            | "max_block_modifications"
            | "max_command_forks"
            | "max_command_sequence_length"
            | "max_entity_cramming"
            | "max_minecart_speed"
            | "max_snow_accumulation_height"
            | "players_nether_portal_creative_delay"
            | "players_nether_portal_default_delay"
            | "players_sleeping_percentage"
            | "random_tick_speed"
            | "respawn_radius"
            | "commandModificationBlockLimit"
            | "maxCommandChainLength"
            | "maxCommandForkCount"
            | "maxEntityCramming"
            | "minecartMaxSpeed"
            | "playersNetherPortalCreativeDelay"
            | "playersNetherPortalDefaultDelay"
            | "playersSleepingPercentage"
            | "randomTickSpeed"
            | "snowAccumulationHeight"
            | "spawnRadius" => Some(Self::Integer),
            "advance_time"
            | "advance_weather"
            | "allow_entering_nether_using_portals"
            | "block_drops"
            | "block_explosion_drop_decay"
            | "command_block_output"
            | "command_blocks_work"
            | "drowning_damage"
            | "elytra_movement_check"
            | "ender_pearls_vanish_on_death"
            | "entity_drops"
            | "fall_damage"
            | "fire_damage"
            | "forgive_dead_players"
            | "freeze_damage"
            | "global_sound_events"
            | "immediate_respawn"
            | "keep_inventory"
            | "lava_source_conversion"
            | "limited_crafting"
            | "locator_bar"
            | "log_admin_commands"
            | "mob_drops"
            | "mob_explosion_drop_decay"
            | "mob_griefing"
            | "natural_health_regeneration"
            | "player_movement_check"
            | "projectiles_can_break_blocks"
            | "pvp"
            | "raids"
            | "reduced_debug_info"
            | "send_command_feedback"
            | "show_advancement_messages"
            | "show_death_messages"
            | "spawn_mobs"
            | "spawn_monsters"
            | "spawn_patrols"
            | "spawn_phantoms"
            | "spawn_wandering_traders"
            | "spawn_wardens"
            | "spawner_blocks_work"
            | "spectators_generate_chunks"
            | "spread_vines"
            | "tnt_explodes"
            | "tnt_explosion_drop_decay"
            | "universal_anger"
            | "water_source_conversion"
            | "allowEnteringNetherUsingPortals"
            | "allowFireTicksAwayFromPlayer"
            | "announceAdvancements"
            | "blockExplosionDropDecay"
            | "commandBlockOutput"
            | "commandBlocksEnabled"
            | "disableElytraMovementCheck"
            | "disablePlayerMovementCheck"
            | "disableRaids"
            | "doDaylightCycle"
            | "doEntityDrops"
            | "doFireTick"
            | "doImmediateRespawn"
            | "doInsomnia"
            | "doLimitedCrafting"
            | "doMobLoot"
            | "doMobSpawning"
            | "doPatrolSpawning"
            | "doTileDrops"
            | "doTraderSpawning"
            | "doVinesSpread"
            | "doWardenSpawning"
            | "doWeatherCycle"
            | "drowningDamage"
            | "enderPearlsVanishOnDeath"
            | "fallDamage"
            | "fireDamage"
            | "forgiveDeadPlayers"
            | "freezeDamage"
            | "globalSoundEvents"
            | "keepInventory"
            | "lavaSourceConversion"
            | "locatorBar"
            | "logAdminCommands"
            | "mobExplosionDropDecay"
            | "mobGriefing"
            | "naturalRegeneration"
            | "projectilesCanBreakBlocks"
            | "reducedDebugInfo"
            | "sendCommandFeedback"
            | "showDeathMessages"
            | "spawnerBlocksEnabled"
            | "spawnMonsters"
            | "spectatorsGenerateChunks"
            | "tntExplodes"
            | "tntExplosionDropDecay"
            | "universalAnger"
            | "waterSourceConversion" => Some(Self::Boolean),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_enumerations_activate() {
        for syntax in [
            "(keepInventory | mod.rule)",
            " (keepInventory|mod.rule) <value> ",
        ] {
            let rule = Gamerule::parse(syntax).unwrap();
            assert_eq!(
                rule.query("").matches().collect::<Vec<_>>(),
                ["keepInventory", "mod.rule"]
            );
        }
        for syntax in [
            "<type> <value>",
            "(onlyOne)",
            "()",
            "(a |)",
            "(a || b)",
            "(a | b) [<value>]",
            "(a | b)<value>",
            "(a | b) <value> [extra]",
            "((a | b))",
            "(<a> | b)",
            "(a b | c)",
            "-> gamerule",
        ] {
            assert!(Gamerule::parse(syntax).is_none(), "{syntax}");
        }
    }

    #[test]
    fn server_membership_is_separate_from_local_value_knowledge() {
        let rule = Gamerule::parse("(keepInventory | randomTickSpeed | mod.rule | 規則)").unwrap();
        assert!(rule.query("kee").diagnostic.is_none());
        assert!(rule.query("kee ").diagnostic.is_some());
        assert!(rule.query("unknown").diagnostic.is_some());
        assert!(rule.query("keep_inventory true").diagnostic.is_some());
        assert!(rule.query("mod.rule anything at all").diagnostic.is_none());
        assert!(rule.query("規則 任意").diagnostic.is_none());
        assert!(rule.query("keepInventory").diagnostic.is_none());
        assert!(rule.query("keepInventory   ").diagnostic.is_none());
        let invalid = rule.query("keepInventory wrong").diagnostic.unwrap();
        assert_eq!(&"keepInventory wrong"[invalid.range], "wrong");
        assert!(rule.query("keepInventory true").diagnostic.is_none());
        assert!(rule.query("keepInventory false").diagnostic.is_none());
        assert!(rule.query("keepInventory TRUE").diagnostic.is_some());
        assert!(rule.query("keepInventory true false").diagnostic.is_some());
    }

    #[test]
    fn integers_use_i64_without_game_specific_ranges() {
        let rule = Gamerule::parse("(random_tick_speed | randomTickSpeed)").unwrap();
        for value in [
            "-9223372036854775808",
            "9223372036854775807",
            "+1",
            "-1",
            "0",
        ] {
            assert!(
                rule.query(&format!("random_tick_speed {value}"))
                    .diagnostic
                    .is_none()
            );
            assert!(
                rule.query(&format!("randomTickSpeed {value}"))
                    .diagnostic
                    .is_none()
            );
        }
        for value in [
            "-9223372036854775809",
            "9223372036854775808",
            "1.5",
            "true",
            "1 2",
        ] {
            assert!(
                rule.query(&format!("randomTickSpeed {value}"))
                    .diagnostic
                    .is_some()
            );
        }
    }
}
