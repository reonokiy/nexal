import json
from dataclasses import MISSING, dataclass, fields
from typing import Any, ClassVar


@dataclass
class FunctionTool:
    name: str
    description: str
    parameters: dict[str, Any]
    params_type: ClassVar[type | None] = None

    def to_openai_tool(self) -> dict[str, Any]:
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        }

    # Map type annotations to expected JSON/Python types.
    _TYPE_MAP: ClassVar[dict[type, tuple[type, ...]]] = {
        str: (str,),
        int: (int,),
        bool: (bool,),
        list: (list,),
        dict: (dict,),
    }

    @classmethod
    def _expected_types(cls, type_annotation: Any) -> tuple[type, ...] | None:
        """Return the expected Python types for a dataclass field annotation."""
        # Direct type object (e.g. str, int, list).
        if type_annotation in cls._TYPE_MAP:
            return cls._TYPE_MAP[type_annotation]
        # Generic alias (e.g. list[str], dict[str, str]).
        origin = getattr(type_annotation, "__origin__", None)
        if origin in cls._TYPE_MAP:
            return cls._TYPE_MAP[origin]
        # String annotation (e.g. "str", "list[str]", from __future__ annotations).
        if isinstance(type_annotation, str):
            base = type_annotation.split("[", 1)[0].split("|", 1)[0].strip()
            for t, expected in cls._TYPE_MAP.items():
                if base == t.__name__:
                    return expected
        return None

    @staticmethod
    def _inner_type(type_annotation: Any) -> tuple[type, ...] | None:
        """Extract inner types from list[X] or dict[K, V] annotations."""
        args = getattr(type_annotation, "__args__", None)
        if args:
            return args
        if isinstance(type_annotation, str):
            if "[" in type_annotation:
                inner = type_annotation.split("[", 1)[1].rstrip("]")
                mapping = {"str": str, "int": int, "bool": bool, "float": float}
                types = tuple(mapping.get(t.strip()) for t in inner.split(","))
                if all(t is not None for t in types):
                    return types  # type: ignore[return-value]
        return None

    @staticmethod
    def _check_inner(param: str, value: Any, container: type, inner: tuple[type, ...]) -> str | None:
        """Validate inner element types for list and dict values."""
        if container is list and len(inner) >= 1:
            for i, item in enumerate(value):
                if not isinstance(item, inner[0]):
                    return f"Parameter '{param}[{i}]' must be {inner[0].__name__}"
        elif container is dict and len(inner) >= 2:
            for dk, dv in value.items():
                if not isinstance(dk, inner[0]):
                    return f"Parameter '{param}' key '{dk}' must be {inner[0].__name__}"
                if not isinstance(dv, inner[1]):
                    return f"Parameter '{param}[{dk}]' must be {inner[1].__name__}"
        return None

    def run(self, arguments: str) -> str:
        if self.params_type is not None:
            try:
                parsed = json.loads(arguments or "{}")
            except json.JSONDecodeError as e:
                return json.dumps({"error": f"Invalid JSON arguments: {e}"})
            if not isinstance(parsed, dict):
                return json.dumps({"error": "Arguments must be a JSON object"})
            valid_fields = {f.name: f for f in fields(self.params_type)}
            # Check required fields (fields without defaults).
            for fname, f in valid_fields.items():
                has_default = (
                    f.default is not MISSING or f.default_factory is not MISSING  # type: ignore[comparison-overlap]
                )
                if not has_default and fname not in parsed:
                    return json.dumps({"error": f"Missing required parameter: '{fname}'"})
            filtered = {}
            for k, v in parsed.items():
                if k not in valid_fields:
                    return json.dumps({"error": f"Unknown parameter: '{k}'"})
                expected = self._expected_types(valid_fields[k].type)
                if expected is not None:
                    # bool is a subclass of int; reject bool when int is expected.
                    if expected == (int,) and isinstance(v, bool):
                        return json.dumps({"error": f"Parameter '{k}' must be an integer"})
                    if not isinstance(v, expected):
                        names = "/".join(t.__name__ for t in expected)
                        return json.dumps({"error": f"Parameter '{k}' must be {names}"})
                    # Validate inner element types for list[T] and dict[K, V].
                    inner = self._inner_type(valid_fields[k].type)
                    if inner is not None:
                        err = self._check_inner(k, v, expected[0], inner)
                        if err:
                            return json.dumps({"error": err})
                filtered[k] = v
            try:
                params = self.params_type(**filtered)
            except TypeError as e:
                return json.dumps({"error": f"Invalid parameters: {e}"})
        else:
            params = None
        return self.execute(params)

    def execute(self, params: Any) -> str:
        raise NotImplementedError

    def close(self) -> None:
        pass
