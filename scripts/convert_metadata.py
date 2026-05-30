#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Utility for converting every meta.json under a directory tree to AIROA metadata
v1.3 (or another specified target version).

Usage examples:
    python convert_metadata.py /path/to/root
    python convert_metadata.py /path/to/root --target-version 1.3
"""

import argparse
import dataclasses
import json
import logging
import sys
import shutil
from pathlib import Path
from typing import Dict, Any, List

from airoa_metadata import (
    MetadataV0_0, MetadataV1_0, MetadataV1_1, MetadataV1_2, MetadataV1_3,
    MetadataLoader
)

# Configure logging
logging.basicConfig(level=logging.INFO, format='%(levelname)s: %(message)s')
logger = logging.getLogger(__name__)

# External git repo mapping for components
GIT_REPO_MAPPING = {
    "data_collection": "https://github.com/airoa-org/hsr_data_collection.git",
    "data_capture": "https://github.com/airoa-org/hsr_data_collection.git", 
    "interface": "https://github.com/airoa-org/hsr_leader_teleop.git"
}


def add_git_uris(data):
    """Add git URIs to components based on their role."""
    if "context" in data and "components" in data["context"]:
        for component in data["context"]["components"]:
            if "source" in component and "git" in component["source"]:
                git_info = component["source"]["git"]
                role = component.get("role", "")
                
                # Add URI if not present or empty
                if not git_info.get("uri"):
                    git_info["uri"] = GIT_REPO_MAPPING.get(role, "")
    
    return data


def add_data_collection_component(data):
    """Add data_collection component if git_branch and git_hash are present."""
    if ("git_branch" in data and "git_hash" in data and 
        data["git_branch"] and data["git_hash"]):
        
        if "context" not in data:
            data["context"] = {}
        if "components" not in data["context"]:
            data["context"]["components"] = []
        
        # Check if data_collection component already exists
        existing_roles = [comp.get("role") for comp in data["context"]["components"]]
        if "data_collection" not in existing_roles:
            data_collection_component = {
                "role": "data_collection",
                "name": "rosbag_manager", 
                "source": {
                    "git": {
                        "uri": GIT_REPO_MAPPING.get("data_collection", ""),
                        "hash": data["git_hash"],
                        "branch": data["git_branch"],
                        "tag": None
                    }
                }
            }
            data["context"]["components"].append(data_collection_component)
    
    return data


def remove_null_values(obj):
    """Recursively remove null values from dictionaries and lists."""
    if isinstance(obj, dict):
        return {k: remove_null_values(v) for k, v in obj.items() if v is not None}
    elif isinstance(obj, list):
        return [remove_null_values(item) for item in obj if item is not None]
    else:
        return obj


def get_version_class(version: str):
    """Get the metadata class for a given version string."""
    version_map = {
        "0.0": MetadataV0_0,
        "1.0": MetadataV1_0,
        "1.1": MetadataV1_1,
        "1.2": MetadataV1_2,
        "1.3": MetadataV1_3,
    }
    
    if version not in version_map:
        available = ", ".join(version_map.keys())
        raise ValueError(f"Unsupported version '{version}'. Available versions: {available}")
    
    return version_map[version]


def compare_versions(v1: str, v2: str) -> int:
    """Compare two version strings. Returns -1 if v1 < v2, 0 if equal, 1 if v1 > v2."""
    def parse_version(v):
        return tuple(map(int, v.split('.')))
    
    parsed_v1 = parse_version(v1)
    parsed_v2 = parse_version(v2)
    
    if parsed_v1 < parsed_v2:
        return -1
    elif parsed_v1 > parsed_v2:
        return 1
    else:
        return 0


def convert_metadata(input_file: Path, target_version: str, output_file: Path = None):
    """
    Convert metadata file to target version.
    
    Args:
        input_file: Path to input JSON file
        target_version: Target version string (e.g., "1.3")
        output_file: Path to output file (if None, outputs to stdout)
    
    Returns:
        Dict containing the converted metadata
    """
    # Load the input file
    try:
        with open(input_file, 'r') as f:
            data = json.load(f)
    except (json.JSONDecodeError, FileNotFoundError) as e:
        raise RuntimeError(f"Error reading input file '{input_file}': {e}") from e
    
    # Get current version from the data
    current_version = data.get("version")
    if not current_version:
        raise ValueError(f"Input file '{input_file}' does not contain a 'version' field")
    
    # Check if conversion is needed
    if current_version == target_version:
        logger.info(f"Input file is already version {target_version}, no conversion needed")
        if output_file:
            # Still copy to output file if specified
            with open(output_file, 'w') as f:
                json.dump(data, f, indent=2)
        else:
            # Output to stdout
            json.dump(data, sys.stdout, indent=2)
        return data
    
    # Check if this is an ascending conversion
    if compare_versions(current_version, target_version) > 0:
        raise ValueError(
            f"Descending conversion from {current_version} to {target_version} is not supported"
        )
    
    # Apply git repo mapping and add data collection component if needed
    data = add_data_collection_component(data)
    data = add_git_uris(data)
    
    # Load metadata using the loader (which auto-detects version)
    try:
        loader = MetadataLoader()
        metadata = loader.load_from_dict(data)
    except Exception as e:
        raise RuntimeError(f"Error loading metadata from '{input_file}': {e}") from e
    
    # Convert to target version
    try:
        target_class = get_version_class(target_version)
        converted = target_class.convert(metadata)
    except Exception as e:
        raise RuntimeError(
            f"Error during conversion from {current_version} to {target_version}: {e}"
        ) from e
    
    # Convert back to dict for output and apply post-processing
    converted_dict = dataclasses.asdict(converted)
    converted_dict = add_git_uris(converted_dict)
    
    # Add $schema field for v1.3
    if target_version == "1.3":
        converted_dict["$schema"] = "https://raw.githubusercontent.com/airoa-org/airoa-metadata/refs/tags/v1.3/airoa_metadata/schemas/v1_3.json"
    
    converted_dict = remove_null_values(converted_dict)
    
    # Write output
    if output_file:
        try:
            with open(output_file, 'w') as f:
                json.dump(converted_dict, f, indent=2)
        except OSError as e:
            raise RuntimeError(f"Error writing output file '{output_file}': {e}") from e
    else:
        # Output to stdout
        json.dump(converted_dict, sys.stdout, indent=2)
    
    return converted_dict


def gather_meta_files(root_path: Path) -> List[Path]:
    """Return all meta.json files under the given root."""
    if not root_path.exists():
        raise FileNotFoundError(f"Root path '{root_path}' does not exist")
    if root_path.is_file():
        if root_path.name != "meta.json":
            raise ValueError(f"File '{root_path}' is not named meta.json")
        return [root_path]

    files = sorted(path for path in root_path.rglob("meta.json") if path.is_file())
    return files


def create_backup(file_path: Path, suffix: str = ".orig") -> Path:
    """Create a numbered backup copy of the given file."""
    counter = 0
    while True:
        suffix_part = f"{suffix}{counter}" if counter else suffix
        backup_path = file_path.with_name(f"{file_path.name}{suffix_part}")
        if not backup_path.exists():
            break
        counter += 1

    shutil.copy2(file_path, backup_path)
    return backup_path


def process_meta_files(root: Path, target_version: str) -> None:
    meta_files = gather_meta_files(root)
    if not meta_files:
        logger.warning("No meta.json files found under %s", root)
        return

    logger.info("Found %d meta.json file(s) under %s", len(meta_files), root)

    for file_path in meta_files:
        logger.info("Converting %s", file_path)
        backup_path = create_backup(file_path)
        try:
            convert_metadata(file_path, target_version, file_path)
            logger.info(
                "Updated %s (backup saved at %s)",
                file_path,
                backup_path,
            )
        except Exception as exc:
            logger.error(
                "Failed to convert %s. Backup remains at %s",
                file_path,
                backup_path,
            )
            raise exc


def main():
    parser = argparse.ArgumentParser(
        description="Recursively convert meta.json files to the desired AIROA metadata version",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python convert_metadata.py /path/to/root
  python convert_metadata.py /path/to/root --target-version 1.3

Supported target versions: 0.0, 1.0, 1.1, 1.2, 1.3
        """
    )
    
    parser.add_argument("root", help="Directory tree (or single meta.json file) to process", type=Path)
    parser.add_argument("--target-version", "-t", default="1.3",
                       help="Target version (e.g., 1.3)")
    
    args = parser.parse_args()
    
    try:
        process_meta_files(args.root, args.target_version)
    except KeyboardInterrupt:
        logger.error("Conversion cancelled by user")
        sys.exit(1)
    except Exception as exc:
        logger.error("Conversion failed: %s", exc)
        sys.exit(1)


if __name__ == "__main__":
    main()
