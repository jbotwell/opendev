//! Animated thinking verb list and fade-in transition logic.
//!
//! Provides 100+ verbs that cycle during LLM processing with a
//! dim-to-bright fade-in color animation.

/// Ticks per character used to compute the fade-in duration.
/// Longer verbs get a proportionally longer fade. At 60ms tick rate,
/// a 10-char verb fades in over 10×2×60ms = 1.2s.
pub const TICKS_PER_CHAR: u64 = 2;

/// Ticks to hold the fully-visible verb before cycling to the next.
/// At 60ms tick rate, this is ~3 seconds.
pub const HOLD_TICKS: u64 = 50;

/// Prime step for pseudo-random verb advancement (avoids sequential cycling).
const VERB_STEP: usize = 37;

/// 100+ thinking verbs for the animated spinner.
pub const THINKING_VERBS: &[&str] = &[
    "Thinking",
    "Pondering",
    "Reasoning",
    "Contemplating",
    "Analyzing",
    "Deliberating",
    "Considering",
    "Evaluating",
    "Processing",
    "Reflecting",
    "Musing",
    "Mulling",
    "Weighing",
    "Computing",
    "Calculating",
    "Formulating",
    "Synthesizing",
    "Deducing",
    "Inferring",
    "Hypothesizing",
    "Investigating",
    "Exploring",
    "Examining",
    "Studying",
    "Reviewing",
    "Assessing",
    "Appraising",
    "Scrutinizing",
    "Parsing",
    "Deciphering",
    "Decoding",
    "Interpreting",
    "Comprehending",
    "Absorbing",
    "Digesting",
    "Distilling",
    "Crystallizing",
    "Brainstorming",
    "Ideating",
    "Conceiving",
    "Imagining",
    "Envisioning",
    "Visualizing",
    "Mapping",
    "Charting",
    "Planning",
    "Strategizing",
    "Architecting",
    "Designing",
    "Structuring",
    "Organizing",
    "Prioritizing",
    "Optimizing",
    "Refining",
    "Polishing",
    "Iterating",
    "Converging",
    "Connecting",
    "Linking",
    "Bridging",
    "Harmonizing",
    "Balancing",
    "Calibrating",
    "Tuning",
    "Aligning",
    "Orchestrating",
    "Assembling",
    "Composing",
    "Crafting",
    "Building",
    "Constructing",
    "Modeling",
    "Simulating",
    "Prototyping",
    "Experimenting",
    "Validating",
    "Verifying",
    "Researching",
    "Probing",
    "Querying",
    "Searching",
    "Surveying",
    "Cataloging",
    "Sorting",
    "Filtering",
    "Curating",
    "Selecting",
    "Extrapolating",
    "Interpolating",
    "Correlating",
    "Aggregating",
    "Abstracting",
    "Generalizing",
    "Speculating",
    "Ruminating",
    "Cogitating",
    "Meditating",
    "Introspecting",
    "Rationalizing",
    "Theorizing",
    "Philosophizing",
    "Conceptualizing",
    "Untangling",
    "Unraveling",
    "Deciphering",
    "Navigating",
    "Traversing",
    "Excavating",
    "Unearthing",
    "Discovering",
    "Uncovering",
    "Illuminating",
    "Elucidating",
    "Clarifying",
    "Demystifying",
    "Simplifying",
    "Consolidating",
    "Integrating",
    "Reconciling",
    "Resolving",
    "Debugging",
    "Diagnosing",
    "Dissecting",
    "Deconstructing",
];

/// Total ticks for one verb's full animation cycle (reveal + hold).
pub fn cycle_ticks_for(verb: &str) -> u64 {
    verb.len() as u64 * TICKS_PER_CHAR + HOLD_TICKS
}

/// Compute fade-in intensity (0.0 = fully dim, 1.0 = fully bright).
///
/// During the fade-in phase, intensity ramps linearly from 0.0 to 1.0.
/// After the fade completes, returns 1.0 for the hold duration.
pub fn compute_fade_intensity(verb: &str, verb_tick: u64) -> f32 {
    let fade_ticks = verb.len() as u64 * TICKS_PER_CHAR;
    if fade_ticks == 0 {
        return 1.0;
    }
    (verb_tick as f32 / fade_ticks as f32).min(1.0)
}

/// Advance to the next verb index using a prime step for pseudo-random feel.
pub fn next_verb_index(current: usize) -> usize {
    (current + VERB_STEP) % THINKING_VERBS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verb_count() {
        assert!(
            THINKING_VERBS.len() >= 100,
            "Expected 100+ verbs, got {}",
            THINKING_VERBS.len()
        );
    }

    #[test]
    fn test_all_verbs_ascii() {
        for verb in THINKING_VERBS {
            assert!(
                verb.is_ascii(),
                "Verb '{}' contains non-ASCII characters",
                verb
            );
        }
    }

    #[test]
    fn test_fade_intensity_progression() {
        let verb = "Pondering"; // 9 chars, fade_ticks = 18
        // Tick 0: intensity = 0.0
        assert!((compute_fade_intensity(verb, 0) - 0.0).abs() < f32::EPSILON);
        // Tick 9: halfway through fade = 0.5
        assert!((compute_fade_intensity(verb, 9) - 0.5).abs() < f32::EPSILON);
        // Tick 18: fully faded in = 1.0
        assert!((compute_fade_intensity(verb, 18) - 1.0).abs() < f32::EPSILON);
        // Tick 50: still 1.0 during hold
        assert!((compute_fade_intensity(verb, 50) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cycle_ticks() {
        let verb = "Thinking"; // 8 chars
        let expected = 8 * TICKS_PER_CHAR + HOLD_TICKS;
        assert_eq!(cycle_ticks_for(verb), expected);
    }

    #[test]
    fn test_no_immediate_repeat() {
        let first = 0;
        let second = next_verb_index(first);
        assert_ne!(first, second);
        let third = next_verb_index(second);
        assert_ne!(second, third);
    }

    #[test]
    fn test_verb_step_visits_all() {
        // With a prime step coprime to the verb count, we should visit all verbs
        let mut visited = std::collections::HashSet::new();
        let mut idx = 0;
        for _ in 0..THINKING_VERBS.len() {
            visited.insert(idx);
            idx = next_verb_index(idx);
        }
        assert_eq!(
            visited.len(),
            THINKING_VERBS.len(),
            "Prime step should visit all verbs"
        );
    }
}
