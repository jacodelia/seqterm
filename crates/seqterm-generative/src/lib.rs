pub mod euclidean;
pub mod markov;

pub use euclidean::{euclidean_rhythm, rotate};
pub use markov::{LcgRng, MarkovChain, MutationEngine, StochasticGate};
