# Source: https://github.com/pydantic/pydantic/blob/v2.11.7/pydantic/deprecated/decorator.py
# License: MIT, https://github.com/pydantic/pydantic/blob/v2.11.7/LICENSE
from typing import Any, Callable, Optional, TypeVar, overload

from typing_extensions import deprecated


AnyCallableT = TypeVar("AnyCallableT", bound=Callable[..., Any])
ConfigType = Optional[type[Any]] | dict[str, Any]


@overload
def validate_arguments(
    func: None = None, *, config: "ConfigType" = None
) -> Callable[["AnyCallableT"], "AnyCallableT"]: ...


@overload
def validate_arguments(func: "AnyCallableT") -> "AnyCallableT": ...


@deprecated("Use validate_call instead.", category=None)
def validate_arguments(
    func: Optional["AnyCallableT"] = None, *, config: "ConfigType" = None
) -> Any:
    def validate(_func: "AnyCallableT") -> "AnyCallableT":
        return _func

    return validate if func is None else validate(func)
