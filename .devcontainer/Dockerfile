FROM mcr.microsoft.com/devcontainers/rust:2-1-trixie

ARG USERNAME=vscode
ARG USER_HOME=/home/${USERNAME}
ARG NVM_VERSION=v0.40.3

ENV DEBIAN_FRONTEND=noninteractive
ENV TERM=xterm-256color
ENV COLORTERM=truecolor
ENV NVM_DIR=/usr/local/share/nvm
ENV PATH=${USER_HOME}/.local/bin:/usr/local/bin:${PATH}

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

USER root

RUN apt-get update \
 && apt-get upgrade -y \
 && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    gnupg \
    htop \
    openssh-client \
    tmux \
    tree \
    vim \
    xz-utils \
    zsh \
 && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /etc/apt/keyrings \
 && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    -o /etc/apt/keyrings/githubcli-archive-keyring.gpg \
 && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
 && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    > /etc/apt/sources.list.d/github-cli.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends gh \
 && rm -rf /var/lib/apt/lists/*

RUN mkdir -p "${NVM_DIR}" \
 && curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/${NVM_VERSION}/install.sh \
    | PROFILE=/dev/null NVM_DIR="${NVM_DIR}" bash \
 && source "${NVM_DIR}/nvm.sh" \
 && nvm install --lts \
 && nvm alias default 'lts/*' \
 && corepack enable \
 && corepack prepare pnpm@latest --activate \
 && node_bin_dir="$(dirname "$(nvm which default)")" \
 && ln -sf "${node_bin_dir}/node" /usr/local/bin/node \
 && ln -sf "${node_bin_dir}/npm" /usr/local/bin/npm \
 && ln -sf "${node_bin_dir}/npx" /usr/local/bin/npx \
 && ln -sf "${node_bin_dir}/corepack" /usr/local/bin/corepack \
 && ln -sf "${node_bin_dir}/pnpm" /usr/local/bin/pnpm

RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_UNMANAGED_INSTALL=/usr/local/bin INSTALLER_NO_MODIFY_PATH=1 sh

RUN npm install -g @anthropic-ai/claude-code

RUN usermod --shell /usr/bin/zsh "${USERNAME}" \
 && mkdir -p "${USER_HOME}/.local/bin" \
 && chown -R "${USERNAME}:${USERNAME}" "${USER_HOME}" \
 && apt-get clean

USER ${USERNAME}

RUN if [[ ! -d "$HOME/.oh-my-zsh/.git" ]]; then \
         rm -rf "$HOME/.oh-my-zsh"; \
         git clone --depth=1 https://github.com/ohmyzsh/ohmyzsh.git "$HOME/.oh-my-zsh"; \
      fi \
 && if [[ ! -f "$HOME/.zshrc" ]]; then \
         cp "$HOME/.oh-my-zsh/templates/zshrc.zsh-template" "$HOME/.zshrc"; \
      fi

RUN gh copilot --help >/dev/null

WORKDIR /workspace
