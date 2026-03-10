"""Chat and query API endpoints."""

from typing import Dict, List

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from opendev.web.state import get_state
from opendev.models.message import ChatMessage, Role
from opendev.web.dependencies.auth import require_authenticated_user

router = APIRouter(
    prefix="/api/chat",
    tags=["chat"],
    dependencies=[Depends(require_authenticated_user)],
)


class QueryRequest(BaseModel):
    """Request model for sending a query."""

    message: str
    sessionId: str | None = None


from opendev.models.api import (
    MessageResponse,
    ToolCallResponse as ToolCallInfo,
    tool_call_to_response as tool_call_to_info,
)


@router.post("/query")
async def send_query(request: QueryRequest) -> Dict[str, str]:
    """Send a query to the AI agent.

    Args:
        request: Query request with message and optional session ID

    Returns:
        Status response

    Raises:
        HTTPException: If query fails
    """
    try:
        state = get_state()

        # Add user message to session
        user_msg = ChatMessage(role=Role.USER, content=request.message)
        state.add_message(user_msg)

        # TODO: Trigger agent processing in background
        # For now, just acknowledge receipt

        return {
            "status": "received",
            "message": "Query processing will be implemented in next phase",
        }

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.get("/messages")
async def get_messages() -> List[MessageResponse]:
    """Get all messages in the current session.

    Returns:
        List of messages

    Raises:
        HTTPException: If retrieval fails
    """
    try:
        state = get_state()

        # Return empty list if no session exists
        session = state.session_manager.get_current_session()
        if not session:
            return []

        messages = state.get_messages()

        return [
            MessageResponse(
                role=msg.role.value,
                content=msg.content,
                timestamp=(
                    msg.timestamp.isoformat()
                    if hasattr(msg, "timestamp") and msg.timestamp
                    else None
                ),
                tool_calls=(
                    [tool_call_to_info(tc) for tc in msg.tool_calls] if msg.tool_calls else None
                ),
                thinking_trace=msg.thinking_trace,
                reasoning_content=msg.reasoning_content,
            )
            for msg in messages
        ]

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


class ClearChatRequest(BaseModel):
    """Request model for clearing chat with optional workspace."""

    workspace: str | None = None


@router.delete("/clear")
async def clear_chat(request: ClearChatRequest | None = None) -> Dict[str, str]:
    """Clear the current chat session.

    Args:
        request: Optional request with workspace path

    Returns:
        Status response

    Raises:
        HTTPException: If clearing fails
    """
    try:
        state = get_state()
        # Create a new session (effectively clearing current one)
        if request and request.workspace:
            state.session_manager.create_session(working_directory=request.workspace)
        else:
            state.session_manager.create_session()

        return {"status": "success", "message": "Chat cleared"}

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/interrupt")
async def interrupt_task() -> Dict[str, str]:
    """Interrupt the currently running task.

    Returns:
        Status response

    Raises:
        HTTPException: If interrupt fails
    """
    try:
        state = get_state()
        # Signal interrupt via state flag (legacy fallback)
        state.request_interrupt()
        # Also signal via ReactExecutor's interrupt token (primary mechanism)
        agent_executor = getattr(state, "_agent_executor", None)
        if agent_executor:
            # Interrupt all running sessions
            for sid in list(agent_executor._current_react_executors.keys()):
                agent_executor.interrupt_session(sid)

        return {"status": "success", "message": "Interrupt requested"}

    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))
