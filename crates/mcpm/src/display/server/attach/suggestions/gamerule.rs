//! Gamerule syntax checker constructed from an explicit enumeration in the server's help output.

use super::syntax::{
    Candidate, CandidateEmphasis, Diagnostic, SyntaxChecker, SyntaxQuery, boolean_argument_query,
    next_token, trailing_range,
};
use std::borrow::Cow;

/// Gamerule argument completer and advisory validator backed by the server's enumerated rule list.
pub(super) struct GameruleSyntax {
    types: Vec<String>,
}

impl GameruleSyntax {
    /// Extracts an explicit `(ruleA | ruleB ...)` gamerule list from help arguments.
    pub fn from_help(arguments: &str) -> Option<Self> {
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

    fn type_candidates(&self) -> Vec<Candidate<'_>> {
        self.types
            .iter()
            .map(|name| Candidate {
                text: name.as_str(),
                description: None,
                emphasis: if ValueType::for_rule(name).is_some() {
                    CandidateEmphasis::Primary
                } else {
                    CandidateEmphasis::Normal
                },
                append_space: true,
            })
            .collect()
    }
}

impl SyntaxChecker for GameruleSyntax {
    fn summary(&self) -> &str {
        "<type> <value>"
    }

    fn query<'a, 'input>(&'a self, arguments: &'input str) -> SyntaxQuery<'a, 'input> {
        let Some(first) = next_token(arguments, 0) else {
            return SyntaxQuery {
                candidates: self.type_candidates(),
                prefix: "",
                replacement: 0..0,
                diagnostic: None,
            };
        };

        let known = self.types.iter().any(|candidate| candidate == first.text);
        if !known {
            let prefix_valid = self
                .types
                .iter()
                .any(|candidate| candidate.starts_with(first.text));
            let diagnostic = if first.complete || !prefix_valid {
                Some(Diagnostic {
                    range: first.range.clone(),
                    message: Cow::Borrowed("Unknown gamerule type"),
                })
            } else {
                None
            };
            return SyntaxQuery {
                candidates: self.type_candidates(),
                prefix: first.text,
                replacement: first.range,
                diagnostic,
            };
        }

        if !first.complete {
            return SyntaxQuery {
                candidates: self.type_candidates(),
                prefix: first.text,
                replacement: first.range,
                diagnostic: None,
            };
        }

        match ValueType::for_rule(first.text) {
            Some(ValueType::Boolean) => boolean_argument_query(arguments, first.range.end),
            Some(ValueType::Integer) => integer_argument_query(arguments, first.range.end),
            None => SyntaxQuery {
                candidates: Vec::new(),
                prefix: "",
                replacement: arguments.len()..arguments.len(),
                diagnostic: None,
            },
        }
    }
}

fn integer_argument_query(arguments: &str, from: usize) -> SyntaxQuery<'static, '_> {
    let Some(token) = next_token(arguments, from) else {
        let end = arguments.len();
        return SyntaxQuery {
            candidates: Vec::new(),
            prefix: "",
            replacement: end..end,
            diagnostic: None,
        };
    };

    if token.text.parse::<i64>().is_err() {
        return SyntaxQuery {
            candidates: Vec::new(),
            prefix: "",
            replacement: token.range.clone(),
            diagnostic: Some(Diagnostic {
                range: token.range,
                message: Cow::Borrowed("Expected a signed 64-bit integer"),
            }),
        };
    }

    if let Some(extra) = next_token(arguments, token.range.end) {
        return SyntaxQuery {
            candidates: Vec::new(),
            prefix: "",
            replacement: extra.range.clone(),
            diagnostic: Some(Diagnostic {
                range: trailing_range(arguments, extra.range.start),
                message: Cow::Borrowed("Unexpected extra argument"),
            }),
        };
    }

    SyntaxQuery {
        candidates: Vec::new(),
        prefix: "",
        replacement: arguments.len()..arguments.len(),
        diagnostic: None,
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
            let rule = GameruleSyntax::from_help(syntax).unwrap();
            let texts: Vec<_> = rule.query("").matches().map(|c| c.text).collect();
            assert_eq!(texts, ["keepInventory", "mod.rule"]);
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
            assert!(GameruleSyntax::from_help(syntax).is_none(), "{syntax}");
        }
    }

    #[test]
    fn boolean_rules_offer_candidates_and_allow_in_progress_prefixes() {
        let rule =
            GameruleSyntax::from_help("(keepInventory | randomTickSpeed | mod.rule | 規則)")
                .unwrap();
        assert!(rule.query("kee").diagnostic.is_none());
        assert!(rule.query("kee ").diagnostic.is_some());
        assert!(rule.query("unknown").diagnostic.is_some());
        assert!(rule.query("keep_inventory true").diagnostic.is_some());
        assert!(rule.query("mod.rule anything at all").diagnostic.is_none());
        assert!(rule.query("規則 任意").diagnostic.is_none());
        assert!(rule.query("keepInventory").diagnostic.is_none());

        let blank_val = rule.query("keepInventory   ");
        assert!(blank_val.diagnostic.is_none());
        assert_eq!(
            blank_val.matches().map(|c| c.text).collect::<Vec<_>>(),
            ["true", "false"]
        );

        let typing = rule.query("keepInventory t");
        assert!(typing.diagnostic.is_none());
        assert_eq!(
            typing.matches().map(|c| c.text).collect::<Vec<_>>(),
            ["true"]
        );

        let finished_prefix = rule.query("keepInventory t ");
        assert!(finished_prefix.diagnostic.is_some());

        let invalid = rule.query("keepInventory wrong").diagnostic.unwrap();
        assert_eq!(&"keepInventory wrong"[invalid.range], "wrong");
        assert!(rule.query("keepInventory true").diagnostic.is_none());
        assert!(rule.query("keepInventory false").diagnostic.is_none());
        assert!(rule.query("keepInventory TRUE").diagnostic.is_some());

        let extra = rule.query("keepInventory true false").diagnostic.unwrap();
        assert_eq!(&"keepInventory true false"[extra.range], "false");
    }

    #[test]
    fn integers_use_i64_without_game_specific_ranges() {
        let rule = GameruleSyntax::from_help("(random_tick_speed | randomTickSpeed)").unwrap();
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
