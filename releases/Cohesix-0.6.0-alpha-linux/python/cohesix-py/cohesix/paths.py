"""Path validation helpers matching Cohesix console limits."""

from __future__ import annotations

from typing import List

from .defaults import DEFAULTS

_SECURE9P = DEFAULTS.get("secure9p", {})
_CONSOLE = DEFAULTS.get("console", {})
MAX_PATH_COMPONENTS = int(_SECURE9P.get("walk_depth", 8))
MAX_PATH_LEN = int(_CONSOLE.get("max_path_len", 96))


def validate_path(path: str) -> List[str]:
    if not path.startswith("/"):
        raise ValueError("paths must be absolute")
    if len(path) > MAX_PATH_LEN:
        raise ValueError(f"path exceeds max length {MAX_PATH_LEN}")
    components = []
    for component in path.split("/")[1:]:
        if not component:
            continue
        if component in (".", ".."):
            raise ValueError(f"path component '{component}' is not permitted")
        if "/" in component:
            raise ValueError("path component contains '/'")
        if "\x00" in component:
            raise ValueError("path component contains NUL byte")
        components.append(component)
        if len(components) > MAX_PATH_COMPONENTS:
            raise ValueError(
                f"path exceeds maximum depth of {MAX_PATH_COMPONENTS} components"
            )
    return components


def join_root(root: str, path: str) -> str:
    components = validate_path(path)
    if root.endswith("/"):
        root = root[:-1]
    if not components:
        return root
    return root + "/" + "/".join(components)
