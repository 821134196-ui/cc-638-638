#!/usr/bin/env bash
# 一条命令启动：课桌椅高度匹配与跨班调拨系统
#   ./run.sh          生产模式（构建前端 + 启动后端，访问 http://localhost:8080）
#   ./run.sh dev      开发模式（Vite 热更新 :5173 + 后端 :8080）
#   PORT=9000 ./run.sh
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-8080}"
export PORT
export DATABASE_URL="${DATABASE_URL:-sqlite://data/deskflow.db}"
export STATIC_DIR="${STATIC_DIR:-frontend/dist}"
export APP_SECRET="${APP_SECRET:-deskflow-dev-secret-change-me}"

# 1) Rust 工具链
if ! command -v cargo >/dev/null 2>&1; then
  if [ -f "$HOME/.cargo/env" ]; then source "$HOME/.cargo/env"; fi
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo ">> 未检测到 Rust，正在通过 rustup 安装…"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  source "$HOME/.cargo/env"
fi

# 2) Node 依赖
if [ ! -d frontend/node_modules ]; then
  echo ">> 安装前端依赖…"
  (cd frontend && npm install --no-audit --no-fund)
fi

mkdir -p data

MODE="${1:-serve}"
if [ "$MODE" = "dev" ]; then
  echo ">> 开发模式：后端 http://localhost:${PORT}，前端 http://localhost:5173（已代理 /api）"
  (cd frontend && npm run dev) &
  FRONT_PID=$!
  trap 'kill $FRONT_PID 2>/dev/null || true' EXIT
  exec cargo run
fi

echo ">> 构建前端…"
(cd frontend && npm run build)

echo ">> 启动后端（首次会编译，请稍候）…"
echo ">> 系统地址：http://localhost:${PORT}"
exec cargo run
