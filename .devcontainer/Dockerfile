FROM mcr.microsoft.com/devcontainers/rust:1

USER vscode
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/home/vscode/.claude/local/bin:$PATH"
