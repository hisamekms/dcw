FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

USER vscode
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/home/vscode/.claude/local/bin:$PATH"
