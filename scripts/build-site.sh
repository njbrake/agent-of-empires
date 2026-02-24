#!/usr/bin/env bash
set -euo pipefail

# Build the full Agent of Empires website
# Output: dist/ directory ready for deployment

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"

echo "Building Agent of Empires website..."

# Clean previous build
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

# 1. Copy shared assets to website/public before Astro build
echo "Copying assets to website..."
mkdir -p "$ROOT_DIR/website/public/assets"
cp "$ROOT_DIR/assets/logo.svg" "$ROOT_DIR/website/public/assets/"
cp "$ROOT_DIR/assets/logo.png" "$ROOT_DIR/website/public/assets/"
cp "$ROOT_DIR/assets/social-preview.png" "$ROOT_DIR/website/public/assets/" 2>/dev/null || true
cp "$ROOT_DIR/assets/social-preview.svg" "$ROOT_DIR/website/public/assets/" 2>/dev/null || true
cp "$ROOT_DIR/theme/favicon.png" "$ROOT_DIR/website/public/assets/" 2>/dev/null || true
if [ -f "$ROOT_DIR/docs/assets/demo.gif" ]; then
  if head -c 6 "$ROOT_DIR/docs/assets/demo.gif" | grep -q "GIF8"; then
    cp "$ROOT_DIR/docs/assets/demo.gif" "$ROOT_DIR/website/public/assets/"
    echo "  - demo.gif copied ($(du -h "$ROOT_DIR/docs/assets/demo.gif" | cut -f1))"
  else
    echo "WARNING: demo.gif appears to be a Git LFS pointer, not actual content"
    echo "  Content: $(head -c 50 "$ROOT_DIR/docs/assets/demo.gif")"
  fi
fi

# 2. Build Astro website
echo "Building Astro website..."
(cd "$ROOT_DIR/website" && npm install && npm run build)

# 3. Copy Astro output to dist/
echo "Copying website..."
cp -r "$ROOT_DIR/website/dist/"* "$DIST_DIR/"

# 4. Copy install script
echo "Copying install script..."
cp "$ROOT_DIR/scripts/install.sh" "$DIST_DIR/"

# 5. Create a simple 404 page that redirects to home
cat > "$DIST_DIR/404.html" << 'EOF'
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Page Not Found - Agent of Empires</title>
  <link rel="icon" type="image/svg+xml" href="/assets/logo.svg">
  <script src="https://cdn.tailwindcss.com"></script>
</head>
<body class="bg-slate-950 text-gray-100 min-h-screen flex items-center justify-center">
  <div class="text-center px-6">
    <img src="/assets/logo.svg" alt="Agent of Empires" class="w-16 h-16 mx-auto mb-6 opacity-50">
    <h1 class="text-4xl font-bold mb-4">404</h1>
    <p class="text-gray-400 mb-8">Page not found</p>
    <a href="/" class="bg-amber-600 hover:bg-amber-500 text-white font-semibold px-6 py-3 rounded-lg transition-colors">
      Go Home
    </a>
  </div>
</body>
</html>
EOF

# 6. Create CNAME file for GitHub Pages (if using custom domain)
echo "agent-of-empires.com" > "$DIST_DIR/CNAME"

echo ""
echo "Build complete! Output in: $DIST_DIR"
echo ""
echo "Directory structure:"
find "$DIST_DIR" -type f -print 2>/dev/null | head -20 | sed "s|$DIST_DIR|dist|" || true
echo ""
echo "To preview locally:"
echo "  cd $DIST_DIR && python3 -m http.server 8000"
echo ""
echo "To deploy to GitHub Pages, Cloudflare Pages, or Netlify:"
echo "  Point your deployment to the dist/ directory"
