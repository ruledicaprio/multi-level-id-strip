"""JSON-repair for small quantized models, which frequently drift: code fences,
prose around the object, and trailing commas. Carried over and hardened from the
original `extract_json.py` cleanup.
"""

import json
import re


def repair_json(raw: str) -> dict:
    """Best-effort parse of a model's JSON output into a dict.

    Strips ```json fences, narrows to the outermost `{...}`, and removes
    trailing commas before a closing brace/bracket. Raises `json.JSONDecodeError`
    if the result still isn't valid JSON.
    """
    s = raw.strip()

    # Strip a leading ```json / ``` fence and a trailing ``` fence.
    if s.startswith("```"):
        s = re.sub(r"^```[a-zA-Z0-9]*\s*", "", s)
        s = re.sub(r"\s*```$", "", s).strip()

    # Narrow to the outermost JSON object if there is surrounding prose.
    start, end = s.find("{"), s.rfind("}")
    if start != -1 and end != -1 and end > start:
        s = s[start : end + 1]

    # Remove trailing commas: `,}` / `,]` (optionally with whitespace between).
    s = re.sub(r",(\s*[}\]])", r"\1", s)

    parsed = json.loads(s)
    if not isinstance(parsed, dict):
        raise json.JSONDecodeError("expected a JSON object", s, 0)
    return parsed
