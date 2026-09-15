#!/usr/bin/env bash

# Exit immediately if a command exits with a non-zero status,
# or if an uninitialized variable is used.
set -euo pipefail

# Verify Docker is available before doing anything else
if ! command -v docker >/dev/null 2>&1; then
    echo "Error: docker is required but not installed or not on PATH." >&2
    exit 1
fi

# Initialize default variable state
DRY_RUN=false
SKIP_CONFIRM=false
MINUTES=""
NO_TIME_FILTER=false
LABEL_KEY="pulse-gate-test"
LABEL_VALUE="true"
NAME_PREFIX="pulse-gate-test-"

# Function to print usage instructions
print_usage() {
    echo "Usage: $0 [options] <minutes>"
    echo ""
    echo "Options:"
    echo "  -d, --dry-run            Show what containers would be affected without changing anything"
    echo "  -y, --yes                Skip the interactive confirmation step and delete immediately"
    echo "  -l, --label KEY VALUE    Only target containers with the given label key/value"
    echo "  -p, --name-prefix PREFIX Only target containers whose names contain the prefix"
    echo "  --no-time-filter         Ignore the time filter and target all matching containers"
    echo "  -h, --help               Show this help message"
    echo ""
    echo "Example:"
    echo "  $0 40"
    echo "  $0 --dry-run 30"
    echo "  $0 -y 15"
    echo "  $0 --label pulse-gate-test true 30"
    echo "  $0 --name-prefix pulse-gate-test- 30"
    echo "  $0 --no-time-filter"
}

# 1. Parse optional flags using a while loop
while [[ $# -gt 0 ]]; do
    case "$1" in
        -d|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -y|--yes)
            SKIP_CONFIRM=true
            shift
            ;;
        -l|--label)
            if [ $# -lt 3 ]; then
                echo "Error: --label requires two arguments: KEY VALUE" >&2
                print_usage
                exit 1
            fi
            LABEL_KEY="$2"
            LABEL_VALUE="$3"
            shift 3
            ;;
        -p|--name-prefix)
            if [ $# -lt 2 ]; then
                echo "Error: --name-prefix requires an argument" >&2
                print_usage
                exit 1
            fi
            NAME_PREFIX="$2"
            shift 2
            ;;
        --no-time-filter)
            NO_TIME_FILTER=true
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        -*)
            echo "Error: Unknown option $1" >&2
            print_usage
            exit 1
            ;;
        *)
            if [ -z "$MINUTES" ]; then
                MINUTES="$1"
                shift
            else
                echo "Error: Unexpected extra argument '$1'" >&2
                print_usage
                exit 1
            fi
            ;;
    esac
done

# 2. Check if the mandatory positional argument was captured
if [ "$NO_TIME_FILTER" = false ] && [ -z "$MINUTES" ]; then
    echo "Error: Missing mandatory time constraint argument (minutes)." >&2
    print_usage
    exit 1
fi

if [ "$NO_TIME_FILTER" = false ]; then
    # 3. Validate that the input is a positive integer
    if [[ ! "$MINUTES" =~ ^[0-9]+$ ]] || [ "$MINUTES" -eq 0 ]; then
        echo "Error: Argument must be a positive integer representing minutes." >&2
        print_usage
        exit 1
    fi
fi

if [ "$DRY_RUN" = true ]; then
    echo "--- DRY RUN MODE ENABLED ---"
    echo "The script will only log targets and skip modifications."
fi

if [ "$NO_TIME_FILTER" = true ]; then
    echo "Scanning for Docker containers matching label/prefix without time filter..."
else
    echo "Scanning for Docker containers created less than $MINUTES minutes ago..."

    # 4. Calculate the boundary threshold timestamp in Epoch Seconds
    if date -d "$MINUTES minutes ago" +%s &>/dev/null; then
        # Linux / GNU date
        THRESHOLD_EPOCH=$(date -d "$MINUTES minutes ago" +%s)
    else
        # macOS / BSD date
        THRESHOLD_EPOCH=$(date -v-"${MINUTES}"M +%s)
    fi
fi

# 5. Extract ID, Image, Names, and Command separated by tabs
RAW_DATA=$(docker ps -a --filter "label=${LABEL_KEY}=${LABEL_VALUE}" --filter "name=${NAME_PREFIX}" --format '{{.ID}}\t{{.Image}}\t{{.Names}}\t{{.Command}}')

# 6. Filter containers mathematically using uniform UTC Epoch Seconds
AFFECTED_CONTAINERS=""

while IFS=$'\t' read -r c_id c_image c_name c_cmd; do
    # Skip processing if line is empty
    [ -z "$c_id" ] && continue

    # Fetch the precise standard UTC timestamp from the inspect API
    # Using JSON formatting guarantees a unified format string
    ISO_CREATED=$(docker inspect -f '{{json .Created}}' "$c_id" | tr -d '"')

    # Convert to a Unix Epoch integer safely across platforms, preserving timezone offsets
    if date -d "$ISO_CREATED" +%s &>/dev/null; then
        # GNU date understands RFC3339 with fractional seconds and offsets
        CONTAINER_EPOCH=$(date -d "$ISO_CREATED" +%s)
    else
        # BSD date needs a fixed format and does not accept colon in %z
        CLEAN_TS=$(echo "$ISO_CREATED" \
            | sed -E 's/\.[0-9]+//g' \
            | sed -E 's/Z$/+0000/' \
            | sed -E 's/([+-][0-9]{2}):([0-9]{2})$/\1\2/')
        CONTAINER_EPOCH=$(date -j -f "%Y-%m-%dT%H:%M:%S%z" "$CLEAN_TS" +%s)
    fi

    # Compare integers mathematically or include all matching containers when --no-time-filter is enabled
    if [ "$NO_TIME_FILTER" = true ] || [ "$CONTAINER_EPOCH" -ge "$THRESHOLD_EPOCH" ]; then
        AFFECTED_CONTAINERS="${AFFECTED_CONTAINERS}${c_id}\t${c_image}\t${c_name}\t${c_cmd}\n"
    fi
done <<< "$RAW_DATA"

# echo "AFFECTED_CONTAINERS 1 -> $AFFECTED_CONTAINERS"
# Remove trailing blank lines safely
AFFECTED_CONTAINERS=$(printf "%b" "$AFFECTED_CONTAINERS" | sed '/^$/d')
# echo "AFFECTED_CONTAINERS 2 -> $AFFECTED_CONTAINERS"

# 7. Check if any containers matched the criteria
if [ -z "$AFFECTED_CONTAINERS" ]; then
    echo "No containers found matching the criteria."
    exit 0
fi

echo ""
echo "Found the following containers matching the criteria:"
echo "----------------------------------------------------------------------------------------------------"
printf "%-14s %-30s %-30s %-35s\n" "CONTAINER ID" "IMAGE" "NAME" "COMMAND"
echo "$AFFECTED_CONTAINERS" | awk -F'\t' '{printf "%-14s %-30s %-30s %-35s\n", $1, $2, $3, $4}'
echo "----------------------------------------------------------------------------------------------------"

# Extract just the raw container IDs from our pool
TARGET_IDS=$(echo "$AFFECTED_CONTAINERS" | awk -F'\t' '{print $1}')

# 8. Evaluate dry-run configuration or execute stop/removal operations
if [ "$DRY_RUN" = true ]; then
    echo "Dry run complete. No containers were modified."
    exit 0
fi

# 9. Handle Interactive Confirmation Steps
if [ "$SKIP_CONFIRM" = false ]; then
    echo ""
    read -r -p "Are you sure you want to STOP and REMOVE these containers? (y/N): " RESPONSE < /dev/tty
    case "$RESPONSE" in
        [yY][eE][sS]|[yY])
            echo "Confirmation received. Proceeding..."
            ;;
        *)
            echo "Operation cancelled by user."
            exit 0
            ;;
    esac
else
    echo "Skipping confirmation step (--yes flag active)..."
fi

# 10. Execute the actual destructive loops
echo ""
for CONTAINER_ID in $TARGET_IDS; do
    echo "Stopping container: $CONTAINER_ID"
    docker stop "$CONTAINER_ID" >/dev/null

    echo "Removing container: $CONTAINER_ID"
    docker rm "$CONTAINER_ID" >/dev/null
done

echo ""
echo "Cleanup completed successfully!"