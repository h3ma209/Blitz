#!/usr/bin/env bash
set -euo pipefail

# Usage: ./scripts/release.sh <tag> [notes_file]
# Example: ./scripts/release.sh v1.0.0 release_notes.md

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <tag> [notes_file]"
    echo "Example: $0 v1.0.0 release_notes.md"
    exit 1
fi

TAG="$1"
NOTES_FILE="${2:-}"

# Check if gh is installed
if ! command -v gh &> /dev/null; then
    echo "Error: GitHub CLI (gh) is not installed. Please install it from https://cli.github.com/"
    exit 1
fi

# Check if we are in a git repository
if ! git rev-parse --is-inside-work-tree &> /dev/null; then
    echo "Error: Not inside a git repository."
    exit 1
fi

# Check if tag already exists
if git rev-parse "refs/tags/$TAG" &> /dev/null; then
    echo "Error: Tag $TAG already exists."
    exit 1
fi

echo "Creating tag $TAG..."
git tag "$TAG"

echo "Pushing tag $TAG to origin..."
git push origin "$TAG"

if [[ -n "$NOTES_FILE" ]]; then
    if [[ ! -f "$NOTES_FILE" ]]; then
        echo "Error: Notes file $NOTES_FILE does not exist."
        exit 1
    fi
    echo "Creating GitHub release for $TAG with notes from $NOTES_FILE..."
    gh release create "$TAG" --title "$TAG" --notes-file "$NOTES_FILE"
else
    echo "Creating GitHub release for $TAG..."
    gh release create "$TAG" --title "$TAG" --generate-notes
fi

echo "Release $TAG created successfully."
