use crate::testing_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "logging" => Some(testing_modules::create_logging_module()),
        "logging.handlers" => Some(testing_modules::create_logging_handlers_module()),
        "logging.config" => Some(testing_modules::create_logging_config_module()),
        "unittest.mock" => Some(testing_modules::create_unittest_mock_module()),
        "pdb" => Some(testing_modules::create_pdb_module()),
        "profile" => Some(testing_modules::create_profile_module()),
        "cProfile" => Some(testing_modules::create_cprofile_module()),
        "timeit" => Some(testing_modules::create_timeit_module()),
        "faulthandler" => Some(testing_modules::create_faulthandler_module()),
        "tracemalloc" => Some(testing_modules::create_tracemalloc_module()),
        "pydoc" => Some(testing_modules::create_pydoc_module()),
        "_testcapi" => Some(testing_modules::create_testcapi_module()),
        "pickletools" => Some(testing_modules::create_pickletools_module()),
        _ => None,
    }
}
