# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.

from importlib.metadata import version as _dist_version

__all__ = ["version", "__version__"]

# The version is set once, in this package's Cargo.toml, and recorded in the
# installed distribution's metadata at build time.
__version__ = version = _dist_version("openjd-model")
