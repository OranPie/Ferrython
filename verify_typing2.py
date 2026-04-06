import typing

# TypeVar with bound kwarg
TB = typing.TypeVar('TB', bound=int)
assert TB.__bound__ is not None
assert TB.__name__ == 'TB'
print("TypeVar bound: OK")

# TypeVar with covariant
T_co = typing.TypeVar('T_co', covariant=True)
assert T_co.__covariant__ == True
assert T_co.__contravariant__ == False
print("TypeVar covariant: OK")

# TypeVar with contravariant
T_contra = typing.TypeVar('T_contra', contravariant=True)
assert T_contra.__contravariant__ == True
assert T_contra.__covariant__ == False
print("TypeVar contravariant: OK")

# InitVar subscript
iv = typing.InitVar[int]
print("InitVar[int]: OK")

# Final subscript
f = typing.Final[str]
print("Final[str]: OK")

print("ALL EXTENDED CHECKS PASSED")
