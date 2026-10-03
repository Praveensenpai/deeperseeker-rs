# /// script
# requires-python = ">=3.11"
# dependencies = [
#     "openai>=1.30.0",
#     "rich>=13.7.0",
# ]
# ///
"""Demonstration client querying DeeperSeeker reverse proxy via OpenAI API."""

import os
import sys

from openai import APIError, OpenAI
from rich.console import Console
from rich.panel import Panel
from rich.rule import Rule
from rich.theme import Theme

custom_theme = Theme(
    {
        "thought": "italic cyan",
        "content": "bold white",
        "tag": "bold magenta",
        "meta": "dim green",
    }
)

console = Console(theme=custom_theme)


def main() -> None:
    """Connect to DeeperSeeker and stream an AI completion."""
    base_url = os.environ.get("DEEPERSEEKER_URL", "http://mochi:4000/v1")
    api_key = os.environ.get("DEEPERSEEKER_API_KEY", "dseeker")
    model = os.environ.get("DEEPERSEEKER_MODEL", "v4.1flash")

    prompt = (
        sys.argv[1]
        if len(sys.argv) > 1
        else "In 3 bullet points, why is Rust's ownership model unique?"
    )

    console.print(
        Panel(
            f"[bold]Target Endpoint:[/bold] [cyan]{base_url}[/cyan]\n"
            f"[bold]Target Model:[/bold]    [yellow]{model}[/yellow]\n"
            f"[bold]Prompt:[/bold]          [white]{prompt}[/white]",
            title="⚡ DeeperSeeker API Client Demo",
            border_style="magenta",
        )
    )

    client = OpenAI(base_url=base_url, api_key=api_key)

    console.print(Rule(title="[meta]Streaming Live Response[/meta]"))

    reasoning_active = False
    first_token = True

    try:
        response = client.chat.completions.create(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            stream=True,
        )

        for chunk in response:
            delta = chunk.choices[0].delta if chunk.choices else None
            if not delta:
                continue

            reasoning = getattr(delta, "reasoning_content", None)
            content = delta.content

            if reasoning:
                if not reasoning_active:
                    console.print("[dim cyan]🧠 Thinking:[/dim cyan] ", end="")
                    reasoning_active = True
                console.print(f"[dim cyan]{reasoning}[/dim cyan]", end="")

            if content:
                if reasoning_active:
                    console.print("\n\n[bold green]✦ Response:[/bold green]\n")
                    reasoning_active = False
                elif first_token:
                    console.print("\n[bold green]✦ Response:[/bold green]\n")
                    first_token = False
                console.print(content, end="")

        console.print("\n")
        console.print(Rule(title="[bold green]✔ Stream Completed Cleanly[/bold green]"))

    except (APIError, OSError) as exc:
        console.print(
            f"\n[bold red]✖ Error communicating with gateway:[/bold red] {exc}"
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
