#!/bin/bash

# regenerate_sqlx_cache.sh
# 
# Script to regenerate SQLx query cache for monero-rpc-pool
# 
# This script:
# 1. Creates a temporary SQLite database in a temp directory
# 2. Runs all database migrations to set up the schema
# 3. Regenerates the SQLx query cache (.sqlx directory)
# 4. Cleans up temporary files automatically
#
# Usage:
#   ./regenerate_sqlx_cache.sh
#
# Requirements:
# - cargo and sqlx-cli must be installed
# - Must be run from the monero-rpc-pool directory
# - migrations/ directory must exist with valid migration files
#
# The generated .sqlx directory should be committed to version control
# to enable offline compilation without requiring DATABASE_URL.

set -e  # Exit on any error

echo "🔄 Regenerating SQLx query cache..."

# Create a temporary directory for the database
TEMP_DIR=$(mktemp -d)
TEMP_DB="$TEMP_DIR/temp_sqlx_cache.sqlite"
DATABASE_URL="sqlite:$TEMP_DB"

echo "📁 Using temporary database: $TEMP_DB"

# Function to cleanup on exit
cleanup() {
    echo "🧹 Cleaning up temporary files..."
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

# Export DATABASE_URL for sqlx commands
export DATABASE_URL

echo "🗄️  Creating database..."
cargo sqlx database create

echo "🔄 Running migrations..."
cargo sqlx migrate run

echo "⚡ Preparing SQLx query cache..."
cargo sqlx prepare

echo "✅ SQLx query cache regenerated successfully!"
echo "📝 The .sqlx directory has been updated with the latest query metadata."
echo "💡 Make sure to commit the .sqlx directory to version control." 