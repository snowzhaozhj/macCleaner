# macCleaner 任务运行器 —— 命令清单见 CLAUDE.md
# `just` 或 `just --list` 查看全部

default:
    @just --list

[group('构建')]
build:
    cargo build --workspace

[group('构建')]
build-release:
    cargo build --release

[group('运行')]
run:
    cargo run -p mc

# 运行 CLI 子命令，如 `just cli clean --dry-run`
[group('运行')]
cli *ARGS:
    cargo run -p mc -- {{ARGS}}

[group('测试 / Lint')]
test *ARGS:
    cargo test --workspace {{ARGS}}

[group('测试 / Lint')]
lint:
    cargo clippy --workspace --all-targets -- -D warnings

[group('测试 / Lint')]
fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

[group('GUI')]
gui-dev:
    cd crates/gui && cargo tauri dev

[group('GUI')]
gui-build:
    cd crates/gui && cargo tauri build

[group('维护')]
gen-rules:
    cargo run -p xtask -- gen-rules

# 本地复现 CI（构建 + 测试 + lint + RULES.md 漂移门禁）
[group('维护')]
ci:
    cargo build --workspace --all-targets
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo run -p xtask -- gen-rules
    git diff --exit-code RULES.md
