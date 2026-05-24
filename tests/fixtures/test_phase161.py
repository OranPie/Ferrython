import sys
import types
import unittest


class LoaderTarget(unittest.TestCase):
    calls = []

    def test_named_method(self):
        type(self).calls.append(self._testMethodName)


module = types.ModuleType("phase161_loader_module")
module.LoaderTarget = LoaderTarget
sys.modules[module.__name__] = module

loader = unittest.TestLoader()

suite = loader.loadTestsFromName(
    "phase161_loader_module.LoaderTarget.test_named_method"
)
assert suite.countTestCases() == 1
result = unittest.TestResult()
suite.run(result)
assert result.wasSuccessful()
assert LoaderTarget.calls == ["test_named_method"]

LoaderTarget.calls = []
suite = loader.loadTestsFromName("LoaderTarget.test_named_method", module)
assert suite.countTestCases() == 1
result = unittest.TestResult()
suite.run(result)
assert result.wasSuccessful()
assert LoaderTarget.calls == ["test_named_method"]

print("phase161 ok")
