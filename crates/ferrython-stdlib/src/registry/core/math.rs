use crate::math_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "math" => Some(math_modules::create_math_module()),
        "statistics" => Some(math_modules::create_statistics_module()),
        "numbers" => Some(math_modules::create_numbers_module()),
        "random" => Some(math_modules::create_random_module()),
        "heapq" => Some(math_modules::create_heapq_module()),
        "bisect" => Some(math_modules::create_bisect_module()),
        "fractions" => Some(math_modules::create_fractions_module()),
        "cmath" => Some(math_modules::create_cmath_module()),
        "colorsys" => Some(math_modules::create_colorsys_module()),
        _ => None,
    }
}
