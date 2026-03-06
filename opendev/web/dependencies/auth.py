"""Authentication dependencies for FastAPI routes."""

from __future__ import annotations

from fastapi import Depends, HTTPException, Request, status

from opendev.models.user import User
from opendev.web.routes.auth import TOKEN_COOKIE, verify_token
from opendev.web.state import get_state


async def require_authenticated_user(request: Request) -> User:
    """Ensure the request has a valid authenticated user."""

    token = request.cookies.get(TOKEN_COOKIE)
    if not token:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Not authenticated")

    user_id = verify_token(token)
    state = get_state()
    user = state.user_store.get_by_id(user_id)
    if not user:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="User not found")

    request.state.user = user
    return user
