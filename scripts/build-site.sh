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

# 1. Build mdbook documentation
echo "Building documentation with mdbook..."
if command -v mdbook &> /dev/null; then
    (cd "$ROOT_DIR" && mdbook build)
else
    echo "Error: mdbook not found. Install with: cargo install mdbook"
    exit 1
fi

# 2. Copy mdbook output to dist/docs/
echo "Copying documentation..."
cp -r "$ROOT_DIR/book" "$DIST_DIR/docs"

# 3. Copy landing page
echo "Copying landing page..."
cp "$ROOT_DIR/website/index.html" "$DIST_DIR/"

# 4. Copy shared assets
echo "Copying assets..."
mkdir -p "$DIST_DIR/assets"
cp "$ROOT_DIR/assets/logo.svg" "$DIST_DIR/assets/"
cp "$ROOT_DIR/assets/logo.png" "$DIST_DIR/assets/"
cp "$ROOT_DIR/assets/social-preview.png" "$DIST_DIR/assets/" 2>/dev/null || true
cp "$ROOT_DIR/assets/social-preview.svg" "$DIST_DIR/assets/" 2>/dev/null || true

# 5. Copy install script
echo "Copying install script..."
cp "$ROOT_DIR/scripts/install.sh" "$DIST_DIR/"

# 6. Create a simple 404 page that redirects to home
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

# 7. Create CNAME file for GitHub Pages (if using custom domain)
echo "agentofempires.com" > "$DIST_DIR/CNAME"

echo ""
echo "Build complete! Output in: $DIST_DIR"
echo ""
echo "Directory structure:"
find "$DIST_DIR" -type f | head -20 | sed "s|$DIST_DIR|dist|"
echo ""
echo "To preview locally:"
echo "  cd $DIST_DIR && python3 -m http.server 8000"
echo ""
echo "To deploy to GitHub Pages, Cloudflare Pages, or Netlify:"
echo "  Point your deployment to the dist/ directory"
