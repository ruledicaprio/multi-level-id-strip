"""Regenerate the gRPC stubs from ../proto/inferer.proto into inferer/.

Run from the `python/` directory: `python generate_grpc.py` (needs grpcio-tools).
The generated `inferer_pb2*.py` files are gitignored; the Docker image and this
script produce them so the runtime protobuf version always matches.
"""

import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).parent
PROTO_DIR = HERE.parent / "proto"
OUT = HERE / "inferer"


def main() -> None:
    subprocess.check_call(
        [
            sys.executable,
            "-m",
            "grpc_tools.protoc",
            f"-I{PROTO_DIR}",
            f"--python_out={OUT}",
            f"--grpc_python_out={OUT}",
            str(PROTO_DIR / "inferer.proto"),
        ]
    )
    # The generated *_grpc.py uses a flat `import inferer_pb2`; rewrite it to a
    # package-relative import so it works as `inferer.inferer_pb2_grpc`.
    grpc_file = OUT / "inferer_pb2_grpc.py"
    text = grpc_file.read_text(encoding="utf-8")
    text = text.replace(
        "import inferer_pb2 as", "from . import inferer_pb2 as"
    )
    grpc_file.write_text(text, encoding="utf-8")
    print("generated inferer_pb2.py + inferer_pb2_grpc.py")


if __name__ == "__main__":
    main()
