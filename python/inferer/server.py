"""Persistent gRPC inference sidecar (Tier 2).

Implements the `Inferer` service from `proto/inferer.proto`: keeps the Qwen GGUF
model warm and answers `Extract`/`Health` RPCs. The Rust pipeline calls this only
when no checksum-valid MRZ exists.

Set `MLIS_INFERER_MOCK=1` to run without the model (returns a canned response) —
used by tests and for wiring checks.
"""

import os
from concurrent import futures

import grpc

from . import inferer_pb2 as pb
from . import inferer_pb2_grpc as pb_grpc
from .adapter import repair_json
from .loader import ModelLoader
from .prompts import build_prompt
from .schemas import Extraction

MOCK = os.environ.get("MLIS_INFERER_MOCK") == "1"


class InfererServicer(pb_grpc.InfererServicer):
    def __init__(self, loader: ModelLoader):
        self.loader = loader

    def Extract(self, request, context):  # noqa: N802 (gRPC method name)
        if MOCK:
            data = {"surname": "MOCK", "document_number": "M0"}
        else:
            raw = self.loader.generate(build_prompt(request.markdown))
            try:
                data = repair_json(raw)
            except Exception as exc:  # invalid JSON from the model
                context.set_code(grpc.StatusCode.INTERNAL)
                context.set_details(f"model produced invalid JSON: {exc}")
                return pb.ExtractResponse()

        # Validate/normalize through the shared schema, then always stamp method.
        model = Extraction(**data)
        model.extraction_method = "llm"

        resp = pb.ExtractResponse(raw_json=model.model_dump_json())
        for field in Extraction.TYPED_FIELDS:
            value = getattr(model, field, None)
            if value is not None:
                setattr(resp, field, value)
        return resp

    def Health(self, request, context):  # noqa: N802 (gRPC method name)
        return pb.HealthReply(
            model_loaded=MOCK or self.loader.loaded,
            model_path=self.loader.model_path,
        )


def build_server(bind: str) -> grpc.Server:
    """Construct (but do not start) the gRPC server bound to `bind`."""
    loader = ModelLoader()
    if not MOCK:
        loader.load()  # warm the model at startup
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    pb_grpc.add_InfererServicer_to_server(InfererServicer(loader), server)
    server.add_insecure_port(bind)
    return server


def serve() -> None:
    bind = os.environ.get("MLIS_INFERER_BIND", "127.0.0.1:50051")
    server = build_server(bind)
    server.start()
    print(f"[mlis-inferer] listening on {bind} (mock={MOCK})", flush=True)
    server.wait_for_termination()


if __name__ == "__main__":
    serve()
