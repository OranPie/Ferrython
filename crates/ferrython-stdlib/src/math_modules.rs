//! Math and statistics stdlib modules

mod bisect;
mod cmath;
mod fractions;
mod functions;
mod heapq;
mod number;
mod numbers;
mod random;
mod statistics;

pub use bisect::create_bisect_module;
pub use cmath::create_cmath_module;
pub use fractions::create_fractions_module;
pub use functions::create_math_module;
pub use heapq::{create_heapq_accel_module, create_heapq_module};
pub use numbers::create_numbers_module;
pub use random::create_random_module;
pub use statistics::create_statistics_module;
