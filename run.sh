#!/usr/bin/env bash
# 一条命令启动：构建前端 → 编译/运行后端（自动建库、写入演示数据、托管前端）。
# 用法：
#   ./run.sh           开发模式（debug，前端已构建则直接启动）
#   ./run.sh --release 生产模式（release 二进制）
#   ./run.sh --fresh   删除旧数据库后重新生成演示数据
set -euo pipefail

cd "$(dirname "$0")"
ROOT="$(pwd)"

MODE="debug"
FRESH=0
for arg in "$@"; do
  case "$arg" in
    --release) MODE="release" ;;
    --fresh)   FRESH=1 ;;
    *) echo "未知参数: $arg" >&2; exit 1 ;;
  esac
done

# ── 1. 前端 ──────────────────────────────────────────────
if [ ! -d frontend/node_modules ]; then
  echo "==> 安装前端依赖（npm install）"
  (cd frontend && npm install)
fi
echo "==> 构建前端（vite → frontend/dist）"
(cd frontend && npm run build)

# ── 2. Rust 工具链（如缺失） ─────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
  if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  else
    echo "==> 未检测到 Rust，正在通过 rustup 安装 stable 工具链"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi
fi

# ── 3. 数据库 ────────────────────────────────────────────
export DATABASE_URL="${DATABASE_URL:-$ROOT/backend/data/desk_match.db}"
export STATIC_DIR="$ROOT/frontend/dist"
export PORT="${PORT:-3000}"
if [ "$FRESH" = "1" ]; then
  echo "==> --fresh：删除旧数据库"
  rm -f "$DATABASE_URL" "$DATABASE_URL-wal" "$DATABASE_URL-shm"
fi
mkdir -p "$(dirname "$DATABASE_URL")"

# ── 4. 启动后端（同时托管前端与 /api） ───────────────────
echo "==> 启动服务：http://localhost:${PORT}"
echo "    班主任: faculty1/faculty1 · faculty2/faculty2 · faculty3/faculty3"
echo "    后勤:   logistics/logistics"
if [ "$MODE" = "release" ]; then
  cargo run --release --manifest-path backend/Cargo.toml
else
  cargo run --manifest-path backend/Cargo.toml
fi
