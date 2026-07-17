"""In-process gRPC smoke test for the inferer, in mock mode (no model needed).

Starts the real grpcio server on loopback and drives it with the generated
client stub — proving the Python stubs, servicer, Pydantic schema and wire
format all work together. Run from `python/`: `python smoke_test.py`.
"""

import os

os.environ.setdefault("MLIS_INFERER_MOCK", "1")

import grpc  # noqa: E402

from inferer import inferer_pb2 as pb  # noqa: E402
from inferer import inferer_pb2_grpc as pb_grpc  # noqa: E402
from inferer.server import build_server  # noqa: E402


def main() -> None:
    bind = "127.0.0.1:50599"
    server = build_server(bind)
    server.start()
    try:
        with grpc.insecure_channel(bind) as chan:
            grpc.channel_ready_future(chan).result(timeout=5)
            stub = pb_grpc.InfererStub(chan)

            resp = stub.Extract(pb.ExtractRequest(markdown="P<UTO passport"))
            assert resp.surname == "MOCK", resp.surname
            assert resp.document_number == "M0", resp.document_number
            assert resp.raw_json and '"extraction_method"' in resp.raw_json
            assert '"llm"' in resp.raw_json

            health = stub.Health(pb.HealthRequest())
            assert health.model_loaded is True, "model_loaded should be true in mock"
        print("OK: inferer gRPC smoke passed")
    finally:
        server.stop(grace=0)


if __name__ == "__main__":
    main()
