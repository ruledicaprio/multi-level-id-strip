"""Pydantic schema mirroring `mlis-core::Extraction` (the LLM-produced fields).

The Tier-1 enrichment fields (`issuing_country_name`, `nationality_name`,
`validity`) are added by the Rust `mrz` crate, not the model, so they are not
part of what the inferer returns.
"""

from typing import ClassVar, Optional

from pydantic import BaseModel, ConfigDict


class Extraction(BaseModel):
    model_config = ConfigDict(extra="ignore")

    document_type: Optional[str] = None
    issuing_country: Optional[str] = None
    document_number: Optional[str] = None
    surname: Optional[str] = None
    given_names: Optional[str] = None
    nationality: Optional[str] = None
    date_of_birth: Optional[str] = None
    sex: Optional[str] = None
    date_of_expiry: Optional[str] = None
    personal_number: Optional[str] = None
    mrz_line: Optional[str] = None
    extraction_method: str = "llm"

    # Fields the Rust ExtractResponse carries as typed columns.
    TYPED_FIELDS: ClassVar[tuple[str, ...]] = (
        "document_type",
        "issuing_country",
        "document_number",
        "surname",
        "given_names",
        "nationality",
        "date_of_birth",
        "sex",
        "date_of_expiry",
        "personal_number",
        "mrz_line",
    )
