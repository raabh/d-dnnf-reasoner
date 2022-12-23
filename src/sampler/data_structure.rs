use crate::sampler::sat_solver::SatSolver;
use std::collections::HashSet;
use std::iter;

/// Represents a (partial) configuration
#[derive(Debug, Clone, Eq, Hash)]
pub struct Config {
    /// A vector of selected features (positive values) and deselected features (negative values)
    literals: Vec<i32>,
    pub sat_state: Option<Vec<bool>>,
    sat_state_complete: bool,
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        self.literals.eq(&other.literals)
    }
}

impl Extend<i32> for Config {
    fn extend<T: IntoIterator<Item=i32>>(&mut self, iter: T) {
        self.sat_state_complete = false;
        self.literals.extend(iter);
        self.literals.sort_unstable();
        self.literals.dedup();
    }
}

impl Config {
    /// Creates a new config with the given literals
    pub fn from(literals: &[i32]) -> Self {
        Self {
            literals: Vec::from(literals),
            sat_state: None,
            sat_state_complete: false,
        }
    }

    /// Creates a new config from two disjoint configs.
    pub fn from_disjoint(left: &Self, right: &Self) -> Self {
        let mut literals = left.literals.clone();
        literals.extend(right.literals.iter());

        let sat_state = match (left.sat_state.clone(), right.sat_state.clone())
        {
            (Some(left_state), Some(right_state)) => {
                /*
                We pick the cached state of the larger config because we can not combine the
                cached states. This would break the upward propagation of the marks.
                Example: There is an AND with two children A and B.
                A is marked in the left state
                B is marked in the right state
                If we simply combine the two states then A is marked and B is marked but the
                marker does not propagate upward to the AND. So the AND remains unmarked which
                is wrong and may cause wrong results when SAT solving.
                 */
                if left.literals.len() >= right.literals.len() {
                    Some(left_state)
                } else {
                    Some(right_state)
                }
            }
            (Some(state), None) | (None, Some(state)) => Some(state),
            (None, None) => None,
        };

        Self {
            literals,
            sat_state,
            sat_state_complete: false, // always false because we can not combine the states
        }
    }

    /// Returns a slice of this configs literals
    pub fn get_literals(&self) -> &[i32] {
        &self.literals
    }

    /// Returns the cached sat state if there is one
    pub fn get_sat_state(&mut self) -> Option<&mut Vec<bool>> {
        self.sat_state.as_mut()
    }

    /// Sets the cached sat state
    pub fn set_sat_state(&mut self, sat_state: Vec<bool>) {
        self.sat_state_complete = true;
        self.sat_state = Some(sat_state);
    }

    /// Returns whether the cached sat state is complete (true) or incomplete (false)
    pub fn is_sat_state_complete(&self) -> bool {
        self.sat_state_complete
    }

    /// Uses the given [SatSolver] to update the cached sat solver state in this config.
    /// This does nothing if the cache is up to date.
    pub fn update_sat_state(&mut self, sat_solver: &SatSolver, root: usize) {
        if self.is_sat_state_complete() {
            debug_assert!(
                self.sat_state.is_some(),
                "sat_state should be Some(_) if sat_state_complete is true"
            );
            return;
        }

        // clone literals to avoid borrow problems in the sat solver call below
        let literals = self.literals.clone();

        if self.sat_state.is_none() {
            self.set_sat_state(sat_solver.new_state());
        }

        sat_solver.is_sat_in_subgraph_cached(
            &literals,
            root,
            &mut self.get_sat_state()
                .expect("sat_state should exist because we initialized it a few lines before"),
        );
    }

    /// Checks if this config obviously conflicts with the interaction.
    /// This is the case when the config contains a literal *l* and the interaction contains *-l*
    pub fn conflicts_with(&self, interaction: &[i32]) -> bool {
        interaction
            .iter()
            .any(|literal| self.literals.contains(&-literal))
    }

    /// Checks if this config covers the given interaction
    pub fn covers(&self, interaction: &[i32]) -> bool {
        interaction
            .iter()
            .all(|literal| self.literals.contains(literal))
    }
}

/// Represents a (partial) sample of configs.
/// The sample differentiates between complete and partial configs.
/// A config is complete (in the context of this sample) if it contains all variables this sample
/// defines. Otherwise the config is partial.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    /// Configs that contain all variables of this sample
    pub complete_configs: Vec<Config>,
    /// Configs that do not contain all variables of this sample
    pub partial_configs: Vec<Config>,
    /// The variables that Configs of this sample may contain
    vars: HashSet<u32>,
    /// The literals that actually occur in this sample
    literals: HashSet<i32>,
}

impl Extend<Config> for Sample {
    fn extend<T: IntoIterator<Item = Config>>(&mut self, iter: T) {
        for config in iter {
            self.add(config);
        }
    }
}

impl Sample {
    /// Create an empty sample that may contain the given variables
    pub fn new(vars: HashSet<u32>) -> Self {
        Self {
            complete_configs: vec![],
            partial_configs: vec![],
            vars,
            literals: HashSet::new(),
        }
    }

    /// Create a new sample that will contain the given configs
    ///
    /// # Examples
    /// ```
    /// use ddnnf_lib::sampler::data_structure::{Config, Sample};
    ///
    /// let conf_a = Config::from(&[1,2]);
    /// let conf_b = Config::from(&[1,2,3]);
    /// let sample = Sample::new_from_configs(vec![conf_a, conf_b]);
    ///
    /// let mut iter = sample.iter();
    /// assert_eq!(Some(&Config::from(&[1,2,3])), iter.next());
    /// assert_eq!(Some(&Config::from(&[1,2])), iter.next());
    /// assert_eq!(None, iter.next());
    /// ```
    pub fn new_from_configs(configs: Vec<Config>) -> Self {
        let literals: HashSet<i32> = configs
            .iter()
            .flat_map(|c| c.literals.iter())
            .cloned()
            .collect();

        let vars: HashSet<u32> =
            literals.iter().map(|x| x.unsigned_abs()).collect();

        let mut sample = Self {
            complete_configs: vec![],
            partial_configs: vec![],
            vars,
            literals,
        };

        sample.extend(configs);
        sample
    }

    pub fn new_from_samples(samples: &[&Self]) -> Self {
        let vars: HashSet<u32> = samples
            .iter()
            .flat_map(|sample| sample.vars.iter())
            .cloned()
            .collect();

        Self::new(vars)
    }

    /// Create an empty sample that may contain the given variables and will certainly contain
    /// the given literals. Only use this if you know that the configs you are going to add to
    /// this sample contain the given literals.
    pub fn new_with_literals(
        vars: HashSet<u32>,
        literals: HashSet<i32>,
    ) -> Self {
        Self {
            complete_configs: vec![],
            partial_configs: vec![],
            vars,
            literals,
        }
    }

    /// Create an empty sample with no variables defined
    pub fn empty() -> Self {
        Self {
            complete_configs: vec![],
            partial_configs: vec![],
            vars: HashSet::new(),
            literals: HashSet::new(),
        }
    }

    /// Create a sample that only contains a single configuration with a single literal
    pub fn from_literal(literal: i32) -> Self {
        let mut sample = Self::new(HashSet::from([literal.unsigned_abs()]));
        sample.add_complete(Config::from(&[literal]));
        sample
    }

    pub fn get_literals(&self) -> &HashSet<i32> {
        &self.literals
    }

    /// Adds a config to this sample. Only use this method if you know that the config is
    /// complete. The added config is treated as a complete config without checking
    /// if it actually is complete.
    pub fn add_complete(&mut self, config: Config) {
        self.literals.extend(&config.literals);
        self.complete_configs.push(config)
    }

    /// Adds a config to this sample. Only use this method if you know that the config is
    /// partial. The added config is treated as a partial config without checking
    /// if it actually is partial.
    pub fn add_partial(&mut self, config: Config) {
        self.literals.extend(&config.literals);
        self.partial_configs.push(config)
    }

    /// Adds a config to this sample and automatically determines whether the config is complete
    /// or partial.
    pub fn add(&mut self, config: Config) {
        debug_assert!(
            config.literals.len() <= self.vars.len(),
            "Can not insert config with more vars than the sample defines"
        );
        if self.is_config_complete(&config) {
            self.add_complete(config)
        } else {
            self.add_partial(config)
        }
    }

    /// Determines whether the config is complete (true) or partial (false).
    pub fn is_config_complete(&self, config: &Config) -> bool {
        debug_assert!(
            config.literals.len() <= self.vars.len(),
            "Can not insert config with more vars than the sample defines"
        );
        config.literals.len() == self.vars.len()
    }

    /// Creates an iterator that first iterates over complete_configs and then over partial_configs
    pub fn iter(&self) -> impl Iterator<Item=&Config> {
        self.complete_configs
            .iter()
            .chain(self.partial_configs.iter())
    }

    pub fn iter_with_completeness(
        &self,
    ) -> impl Iterator<Item = (&Config, bool)> {
        let partial_iter = self.partial_configs.iter().zip(iter::repeat(false));

        self.complete_configs
            .iter()
            .zip(iter::repeat(true))
            .chain(partial_iter)
    }

    /// Returns the number of configs in this sample
    pub fn len(&self) -> usize {
        self.complete_configs.len() + self.partial_configs.len()
    }

    /// Returns true if the sample contains no configs
    ///
    /// # Examples
    /// ```
    /// use std::collections::HashSet;
    /// use ddnnf_lib::sampler::data_structure::{Config, Sample};
    /// let mut s = Sample::new(HashSet::from([1,2,3]));
    ///
    /// assert!(s.is_empty());
    /// s.add_partial(Config::from(&[1,3]));
    /// assert!(!s.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.complete_configs.is_empty() && self.partial_configs.is_empty()
    }

    /// Checks if this sample covers the given interaction
    pub fn covers(&self, interaction: &[i32]) -> bool {
        self.iter().any(|conf| conf.covers(interaction))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::parser::build_ddnnf_tree_with_extras;

    #[test]
    fn test_sample_covering() {
        let sample = Sample {
            complete_configs: vec![Config::from(&vec![1, 2, 3, -4, -5])],
            partial_configs: vec![],
            vars: HashSet::from([1, 2, 3, 4, 5]),
            literals: HashSet::from([1, 2, 3, -4, -5]),
        };

        let covered_interaction = vec![1, 2, -4];
        assert!(sample.covers(&covered_interaction));

        let uncovered_interaction = vec![1, 2, 4];
        assert!(!sample.covers(&uncovered_interaction));
    }

    #[test]
    fn test_cache_updating() {
        let ddnnf =
            build_ddnnf_tree_with_extras("./tests/data/small_test.dimacs.nnf");
        let root = ddnnf.number_of_nodes - 1;
        let sat_solver = SatSolver::new(&ddnnf);

        // expected outcomes
        let expected_for_3 = vec![
            true, false, false, true, false, false, false, false, false, false,
            false, false, false, false, false, false, false,
        ];
        let expected_for_2_3 = vec![
            true, true, false, true, false, false, false, false, false, false,
            true, false, false, false, false, false, false,
        ];

        // config without sat state
        let mut config = Config::from(&vec![3]);
        assert_eq!(config.sat_state, None);

        // update from None to Some(_)
        config.update_sat_state(&sat_solver, root);
        assert_eq!(config.sat_state, Some(expected_for_3));

        // extend the config then update from Some(_) to Some(_)
        config.extend([2]);
        config.update_sat_state(&sat_solver, root);
        assert_eq!(config.sat_state, Some(expected_for_2_3));
    }
}
