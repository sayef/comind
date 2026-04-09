"""
Entry point for both `python -m comind` and the `comind` CLI command.
"""

from comind.cli import app


def main() -> None:
    app()


if __name__ == "__main__":
    main()
