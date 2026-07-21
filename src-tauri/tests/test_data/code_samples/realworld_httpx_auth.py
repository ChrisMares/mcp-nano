# Source: https://github.com/encode/httpx/blob/0.28.1/httpx/_auth.py
# License: BSD-3-Clause, https://github.com/encode/httpx/blob/0.28.1/LICENSE.md
from __future__ import annotations

import typing

from ._models import Request, Response


class Auth:
    def auth_flow(self, request: Request) -> typing.Generator[Request, Response, None]:
        yield request

    async def async_auth_flow(
        self, request: Request
    ) -> typing.AsyncGenerator[Request, Response]:
        flow = self.auth_flow(request)
        request = next(flow)
        while True:
            response = yield request
            try:
                request = flow.send(response)
            except StopIteration:
                break


class FunctionAuth(Auth):
    def __init__(self, func: typing.Callable[[Request], Request]) -> None:
        self._func = func

    def auth_flow(self, request: Request) -> typing.Generator[Request, Response, None]:
        yield self._func(request)
