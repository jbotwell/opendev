"""Mixin for nested tool tracking, tree structure, and animation."""

from __future__ import annotations

import sys
import threading
import time
from typing import TYPE_CHECKING, Optional

from rich.text import Text
from textual.strip import Strip

from opendev.ui_textual.style_tokens import (
    ERROR,
    GREEN_GRADIENT,
    GREY,
    SUBTLE,
    SUCCESS,
)
from opendev.ui_textual.widgets.conversation.tool_renderer.types import (
    TREE_BRANCH,
    TREE_LAST,
    TREE_VERTICAL,
    NestedToolState,
    SingleAgentToolRecord,
)

if TYPE_CHECKING:
    pass


class NestedToolMixin:
    """Nested tool call tracking, tree structure rendering, and animation."""

    # Attributes expected from DefaultToolRenderer.__init__:
    #   log, app, _spacing, _nested_tools, _nested_tool_timer,
    #   _nested_tool_thread_timer, _nested_spinner_char,
    #   _nested_color_index, _nested_tool_line, _nested_tool_text,
    #   _nested_tool_depth, _nested_tool_timer_start,
    #   _parallel_group, _parallel_expanded, _agent_spinner_states,
    #   _single_agent, _header_spinner_index, _bullet_gradient_index,
    #   _spinner_chars, _paused_for_resize, _interrupted,
    #   _text_to_strip (method), _update_agent_row (method),
    #   _update_status_line (method), _update_parallel_header (method),
    #   _update_agent_row_gradient (method),
    #   _update_header_spinner (method from single agent display in main)

    # --- Nested Tool Calls ---

    def add_nested_tool_call(
        self,
        display: Text | str,
        depth: int,
        parent: str,
        tool_id: str = "",
        is_last: bool = False,
    ) -> None:
        """Add a nested tool call with multi-tool tracking support.

        Args:
            display: Tool display text
            depth: Nesting depth (1 = direct child)
            parent: Parent agent name
            tool_id: Unique tool call ID for tracking parallel tools
            is_last: Whether this is the last tool in its group (for tree connectors)
        """
        if self._interrupted:
            return

        if self._parallel_group is not None:
            print(f"[DEBUG PARALLEL] add_nested_tool_call: parent={parent!r}", file=sys.stderr)
            print(
                f"[DEBUG PARALLEL] agents keys={list(self._parallel_group.agents.keys())}",
                file=sys.stderr,
            )
            agent = self._parallel_group.agents.get(parent)
            print(f"[DEBUG PARALLEL] agent found={agent is not None}", file=sys.stderr)
            if agent:
                print(
                    f"[DEBUG PARALLEL] agent.tool_call_id={agent.tool_call_id!r}", file=sys.stderr
                )

        if isinstance(display, Text):
            tool_text = display.copy()
        else:
            tool_text = Text(str(display), style=SUBTLE)

        # If single agent is active, track its tools and update display
        if self._single_agent is not None and self._single_agent.status == "running":
            plain_text = tool_text.plain if hasattr(tool_text, "plain") else str(tool_text)
            if ":" in plain_text:
                tool_name = plain_text.split(":")[0].strip()
            elif "(" in plain_text:
                tool_name = plain_text.split("(")[0].strip()
            else:
                tool_name = plain_text.split()[0] if plain_text.split() else "unknown"

            self._single_agent.tool_count += 1
            self._single_agent.current_tool = plain_text
            self._single_agent.tool_records.append(
                SingleAgentToolRecord(
                    tool_name=tool_name,
                    display_text=plain_text,
                )
            )

            self._update_header_spinner()
            self._update_single_agent_tool_line()
            return

        # If active parallel group: update agent stats and status line in-place
        if self._parallel_group is not None:
            agent = self._parallel_group.agents.get(parent)
            if agent is not None:
                plain_text = tool_text.plain if hasattr(tool_text, "plain") else str(tool_text)
                if ":" in plain_text:
                    tool_name = plain_text.split(":")[0].strip()
                elif "(" in plain_text:
                    tool_name = plain_text.split("(")[0].strip()
                else:
                    tool_name = plain_text.split()[0] if plain_text.split() else "unknown"

                agent.tool_count += 1
                agent.current_tool = plain_text

                self._update_agent_row(agent)
                self._update_status_line(agent)

                if not self._parallel_expanded:
                    return  # DON'T write individual tool line when collapsed

        # Expanded mode: write the tool call line
        self._spacing.before_nested_tool_call()

        formatted = Text()
        indent = self._build_tree_indent(depth, parent, is_last)
        formatted.append(indent)
        formatted.append(f"{self._nested_spinner_char} ", style=GREEN_GRADIENT[0])
        formatted.append_text(tool_text)
        formatted.append(" (0s)", style=GREY)

        self.log.write(formatted, scroll_end=True, animate=False, wrappable=False)

        if not tool_id:
            tool_id = f"{parent}_{len(self._nested_tools)}_{time.monotonic()}"

        key = (parent, tool_id)
        self._nested_tools[key] = NestedToolState(
            line_number=len(self.log.lines) - 1,
            tool_text=tool_text.copy(),
            depth=depth,
            timer_start=time.monotonic(),
            color_index=0,
            parent=parent,
            tool_id=tool_id,
        )

        # Maintain legacy single-tool state for backwards compat
        self._nested_tool_line = len(self.log.lines) - 1
        self._nested_tool_text = tool_text.copy()
        self._nested_tool_depth = depth
        self._nested_color_index = 0
        self._nested_tool_timer_start = time.monotonic()

        self._start_nested_tool_timer()

    def _build_tree_indent(self, depth: int, parent: str, is_last: bool) -> str:
        """Build tree connector prefix for nested tool display.

        Args:
            depth: Nesting depth
            parent: Parent agent name
            is_last: Whether this is the last tool in its group

        Returns:
            String like "   ├─ " or "   └─ " or "   │  ├─ "
        """
        if depth == 1:
            connector = TREE_LAST if is_last else TREE_BRANCH
            return f"   {connector} "
        else:
            return (
                "   "
                + f"{TREE_VERTICAL}  " * (depth - 1)
                + (f"{TREE_LAST} " if is_last else f"{TREE_BRANCH} ")
            )

    def complete_nested_tool_call(
        self,
        tool_name: str,
        depth: int,
        parent: str,
        success: bool,
        tool_id: str = "",
    ) -> None:
        """Complete a nested tool call, updating the display.

        Args:
            tool_name: Name of the tool
            depth: Nesting depth
            parent: Parent agent name
            success: Whether the tool succeeded
            tool_id: Unique tool call ID for tracking
        """
        if self._interrupted:
            return

        # Update single agent tool records with completion status
        if self._single_agent is not None and self._single_agent.tool_records:
            for record in reversed(self._single_agent.tool_records):
                if record.tool_name == tool_name and record.success is True:
                    record.success = success
                    break

        # Try to find the tool in multi-tool tracking dict
        state: Optional[NestedToolState] = None

        if tool_id:
            key = (parent, tool_id)
            state = self._nested_tools.pop(key, None)

        # Fallback: find most recent tool for this parent
        if state is None:
            for key in list(self._nested_tools.keys()):
                if key[0] == parent:
                    state = self._nested_tools.pop(key)
                    break

        # Final fallback: use legacy single-tool state
        if state is None:
            if self._nested_tool_line is None or self._nested_tool_text is None:
                return
            state = NestedToolState(
                line_number=self._nested_tool_line,
                tool_text=self._nested_tool_text,
                depth=self._nested_tool_depth,
                timer_start=self._nested_tool_timer_start or time.monotonic(),
                parent=parent,
            )
            self._nested_tool_line = None
            self._nested_tool_text = None
            self._nested_tool_timer_start = None

        # Stop timers only if no more active tools
        if not self._nested_tools:
            if self._nested_tool_timer:
                self._nested_tool_timer.stop()
                self._nested_tool_timer = None
            if self._nested_tool_thread_timer:
                self._nested_tool_thread_timer.cancel()
                self._nested_tool_thread_timer = None

        # Build completed tool display
        formatted = Text()
        indent = self._build_tree_indent(state.depth, state.parent, is_last=False)
        formatted.append(indent)

        status_char = "\u2713" if success else "\u2717"
        status_color = SUCCESS if success else ERROR

        formatted.append(f"{status_char} ", style=status_color)
        formatted.append_text(state.tool_text)

        elapsed = round(time.monotonic() - state.timer_start)
        formatted.append(f" ({elapsed}s)", style=GREY)

        # In-place update
        from rich.console import Console

        console = Console(width=1000, force_terminal=True, no_color=False)
        segments = list(formatted.render(console))
        strip = Strip(segments)

        if state.line_number < len(self.log.lines):
            self.log.lines[state.line_number] = strip
            self.log.refresh_line(state.line_number)

    def _start_nested_tool_timer(self) -> None:
        """Start or continue the nested tool animation timer."""
        if self._nested_tool_timer is None:
            self._animate_nested_tool_spinner()

    def _animate_nested_tool_spinner(self) -> None:
        """Animate ALL active nested tool spinners AND agent row spinners."""
        if self._paused_for_resize:
            return

        if self._nested_tool_thread_timer:
            self._nested_tool_thread_timer.cancel()
            self._nested_tool_thread_timer = None

        has_active_tools = self._nested_tools or (
            self._nested_tool_line is not None or self._nested_tool_text is not None
        )
        has_active_agents = self._parallel_group is not None and any(
            a.status == "running" for a in self._parallel_group.agents.values()
        )
        has_single_agent = self._single_agent is not None and self._single_agent.status == "running"

        if not has_active_tools and not has_active_agents and not has_single_agent:
            self._nested_tool_timer = None
            return

        # Animate all tools in the multi-tool tracking dict
        for key, state in self._nested_tools.items():
            state.color_index = (state.color_index + 1) % len(GREEN_GRADIENT)
            self._render_nested_tool_line_for_state(state)

        # Also animate legacy single-tool state if present
        if self._nested_tool_line is not None and self._nested_tool_text is not None:
            self._nested_color_index = (self._nested_color_index + 1) % len(GREEN_GRADIENT)
            self._render_nested_tool_line()

        # Animate parallel agents: header spinner and agent row gradient bullets
        if self._parallel_group is not None:
            if any(a.status == "running" for a in self._parallel_group.agents.values()):
                self._header_spinner_index += 1
                self._update_parallel_header()

            for tool_call_id, agent in self._parallel_group.agents.items():
                if agent.status == "running":
                    idx = self._agent_spinner_states.get(tool_call_id, 0)
                    idx = (idx + 1) % len(GREEN_GRADIENT)
                    self._agent_spinner_states[tool_call_id] = idx
                    self._update_agent_row_gradient(agent, idx)

        # Animate single agent: header spinner
        if self._single_agent is not None and self._single_agent.status == "running":
            self._update_header_spinner()

        # Schedule next animation frame
        interval = 0.15
        self._nested_tool_timer = self.log.set_timer(interval, self._animate_nested_tool_spinner)
        self._nested_tool_thread_timer = threading.Timer(interval, self._on_nested_tool_thread_tick)
        self._nested_tool_thread_timer.daemon = True
        self._nested_tool_thread_timer.start()

    def _on_nested_tool_thread_tick(self) -> None:
        """Thread timer callback for nested tool animation."""
        if not self._nested_tools and self._nested_tool_line is None:
            return
        try:
            if self.app:
                self.app.call_from_thread(self._animate_nested_tool_spinner)
        except Exception:
            pass

    def _render_nested_tool_line_for_state(self, state: NestedToolState) -> None:
        """Render a specific nested tool line from its state.

        Args:
            state: The NestedToolState to render
        """
        if state.line_number >= len(self.log.lines):
            return

        elapsed = round(time.monotonic() - state.timer_start)

        formatted = Text()
        indent = self._build_tree_indent(state.depth, state.parent, is_last=False)
        formatted.append(indent)
        color = GREEN_GRADIENT[state.color_index]
        formatted.append(f"{self._nested_spinner_char} ", style=color)
        formatted.append_text(state.tool_text.copy())
        formatted.append(f" ({elapsed}s)", style=GREY)

        from rich.console import Console

        console = Console(width=1000, force_terminal=True, no_color=False)
        segments = list(formatted.render(console))
        strip = Strip(segments)

        self.log.lines[state.line_number] = strip
        self.log.refresh_line(state.line_number)

    def _render_nested_tool_line(self) -> None:
        """Render the legacy single nested tool line."""
        if self._nested_tool_line is None or self._nested_tool_text is None:
            return

        if self._nested_tool_line >= len(self.log.lines):
            return

        elapsed = 0
        if self._nested_tool_timer_start:
            elapsed = round(time.monotonic() - self._nested_tool_timer_start)

        formatted = Text()
        indent = "  " * self._nested_tool_depth
        formatted.append(indent)
        color = GREEN_GRADIENT[self._nested_color_index]
        formatted.append(f"{self._nested_spinner_char} ", style=color)
        formatted.append_text(self._nested_tool_text.copy())
        formatted.append(f" ({elapsed}s)", style=GREY)

        from rich.console import Console

        console = Console(width=1000, force_terminal=True, no_color=False)
        segments = list(formatted.render(console))
        strip = Strip(segments)

        self.log.lines[self._nested_tool_line] = strip
        self.log.refresh_line(self._nested_tool_line)
        if self.app and hasattr(self.app, "refresh"):
            self.app.refresh()
