# Jetson Python compatibility assessment

Status: source/lock assessment complete; native ARM64 build and execution are
pending a separately authorized artifact/HIL job on `drone-flasher`.

The locked project declares Python `>=3.10,<3.15`. Both pinned upstream projects
declare Python `>=3.10`; their metadata lists CPython 3.10 through 3.14. The
selected code is not architecture-specific Python-only code: pyXCP and pya2ldb
both build native extensions with scikit-build-core, CMake, pybind11 2.13.6 and
a C++ compiler. Their declared runtime dependencies include packages with
platform wheels and/or native builds (notably NumPy). Therefore successful
x86_64 lock/build/tests establish source and resolver compatibility only, not a
Jetson ARM64 installation result.

The lock contains platform-neutral resolution for the supported Python range;
uv selects artifacts for the executing platform. Before hardware use, the
serialized Jetson artifact job must record:

```sh
uname -m
python3 --version
cc --version
cmake --version
uv --version
uv sync --frozen --extra test
uv run --frozen python -c 'import pyxcp, pya2l; print(pyxcp.__version__)'
uv run --frozen pytest -q
```

It must also retain the native build log and environment metadata. No x86_64
wheel or virtual environment is transferable as an ARM64 artifact. Physical
serial execution additionally requires the fixture's reviewed port/reset/BSL
procedure; this server task performs no device operation.
