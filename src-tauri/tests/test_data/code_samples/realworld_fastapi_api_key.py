# Source: https://github.com/fastapi/fastapi/blob/0.115.12/fastapi/security/api_key.py
# License: MIT, https://github.com/fastapi/fastapi/blob/0.115.12/LICENSE
from typing import Optional

from fastapi.openapi.models import APIKey, APIKeyIn
from fastapi.security.base import SecurityBase
from starlette.exceptions import HTTPException
from starlette.requests import Request
from typing_extensions import Annotated, Doc


class APIKeyBase(SecurityBase):
    @staticmethod
    def check_api_key(api_key: Optional[str], auto_error: bool) -> Optional[str]:
        if not api_key and auto_error:
            raise HTTPException(status_code=403, detail="Not authenticated")
        return api_key


class APIKeyQuery(APIKeyBase):
    def __init__(
        self,
        *,
        name: Annotated[str, Doc("Query parameter name.")],
        scheme_name: Annotated[Optional[str], Doc("Security scheme name.")] = None,
        description: Annotated[Optional[str], Doc("Security scheme description.")] = None,
        auto_error: Annotated[bool, Doc("Cancel if missing.")] = True,
    ):
        self.model: APIKey = APIKey(**{"in": APIKeyIn.query}, name=name)
        self.scheme_name = scheme_name or self.__class__.__name__
        self.auto_error = auto_error

    async def __call__(self, request: Request) -> Optional[str]:
        api_key = request.query_params.get(self.model.name)
        return self.check_api_key(api_key, self.auto_error)
