"""Main SessionManager class."""

import json
from pathlib import Path
from typing import Optional

from opendev.models.session import Session

from opendev.core.context_engineering.history.session_manager.index import IndexMixin
from opendev.core.context_engineering.history.session_manager.persistence import PersistenceMixin
from opendev.core.context_engineering.history.session_manager.listing import ListingMixin


class SessionManager(IndexMixin, PersistenceMixin, ListingMixin):
    """Manages session persistence and retrieval.

    Sessions are stored in project-scoped directories under
    ``~/.opendev/projects/{encoded-path}/``.

    A lightweight ``sessions-index.json`` file caches session metadata so that
    ``list_sessions()`` is O(1) reads instead of O(N) full-file parses. The
    index is self-healing: if it is missing or corrupted, it is transparently
    rebuilt from the individual session ``.json`` files.
    """

    def __init__(
        self,
        *,
        session_dir: Optional[Path] = None,
        working_dir: Optional[Path] = None,
    ):
        """Initialize session manager.

        Args:
            session_dir: Explicit directory override (tests, ``OPENDEV_SESSION_DIR``).
            working_dir: Working directory used to compute the project-scoped
                session directory via :func:`paths.project_sessions_dir`.

        If neither argument is given, falls back to
        ``~/.opendev/projects/-unknown-/``.
        """
        if session_dir is not None:
            self.session_dir = Path(session_dir).expanduser()
        elif working_dir is not None:
            from opendev.core.paths import get_paths

            paths = get_paths()
            self.session_dir = paths.project_sessions_dir(working_dir)
        else:
            from opendev.core.paths import get_paths, FALLBACK_PROJECT_DIR_NAME

            paths = get_paths()
            self.session_dir = paths.global_projects_dir / FALLBACK_PROJECT_DIR_NAME

        self.session_dir.mkdir(parents=True, exist_ok=True)
        self.current_session: Optional[Session] = None
        self.turn_count = 0

    def get_current_session(self) -> Optional[Session]:
        """Get the current active session."""
        return self.current_session

    @staticmethod
    def generate_title(messages: list[dict]) -> str:
        """Generate a short title from the first user message.

        Simple heuristic: extract the first sentence, truncate to 50 chars.
        No LLM call required.

        Args:
            messages: List of message dicts with 'role' and 'content' keys

        Returns:
            A concise title string (max 50 chars)
        """
        for msg in messages:
            if msg.get("role") == "user":
                content = msg.get("content", "").strip()
                if not content:
                    continue
                # Take first sentence (or first line)
                for sep in (".", "\n", "?", "!"):
                    idx = content.find(sep)
                    if 0 < idx < 80:
                        content = content[:idx]
                        break
                title = content[:50].strip()
                return title if title else "Untitled"
        return "Untitled"

    def set_title(self, session_id: str, title: str) -> None:
        """Set the title for a session.

        Args:
            session_id: Session ID to update
            title: Title to set (max 50 chars)
        """
        title = title[:50]

        # Update in-memory if it's the current session
        if self.current_session and self.current_session.id == session_id:
            self.current_session.metadata["title"] = title
            self.save_session()
            return

        # Otherwise load, update, save on disk
        session_file = self.session_dir / f"{session_id}.json"
        if not session_file.exists():
            return

        with open(session_file) as f:
            data = json.load(f)

        if "metadata" not in data:
            data["metadata"] = {}
        data["metadata"]["title"] = title

        with open(session_file, "w") as f:
            json.dump(data, f, indent=2, default=str)

        # Update the index for the on-disk-only path
        try:
            session = self._load_from_file(session_file)
            self._update_index_entry(session)
        except Exception:
            pass
