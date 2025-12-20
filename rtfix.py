#!/usr/bin/env python3
"""
rtfix - Fix SIMH tape images with extra tape marks at end

SIMH tape images should end with exactly a double tape mark (two consecutive
0x00000000 words = 8 bytes of zeros). Some tools incorrectly add extra tape
marks. This script detects and fixes that issue.
"""

import argparse
import os
import sys
from pathlib import Path


TAPE_MARK = b'\x00\x00\x00\x00'
DOUBLE_TAPE_MARK = TAPE_MARK + TAPE_MARK


def count_trailing_tape_marks(data: bytes) -> int:
    """Count how many consecutive tape marks are at the end of the data."""
    count = 0
    pos = len(data)
    
    while pos >= 4:
        if data[pos-4:pos] == TAPE_MARK:
            count += 1
            pos -= 4
        else:
            break
    
    return count


def check_file(filepath: Path) -> tuple[bool, int]:
    """
    Check if a file has extra tape marks at the end.
    
    Returns:
        (needs_fix, trailing_tm_count)
    """
    with open(filepath, 'rb') as f:
        # Read the last 64 bytes (enough to check for many tape marks)
        f.seek(0, 2)  # End of file
        size = f.tell()
        
        # Read last chunk
        read_size = min(64, size)
        f.seek(-read_size, 2)
        tail = f.read(read_size)
    
    tm_count = count_trailing_tape_marks(tail)
    return (tm_count > 2, tm_count)


def fix_file(filepath: Path, output: Path | None = None, dry_run: bool = False) -> bool:
    """
    Fix a tape image by removing extra tape marks.
    
    Args:
        filepath: Input file path
        output: Output file path (None = modify in place)
        dry_run: If True, don't actually modify anything
        
    Returns:
        True if file was (or would be) modified
    """
    with open(filepath, 'rb') as f:
        data = f.read()
    
    tm_count = count_trailing_tape_marks(data)
    
    if tm_count <= 2:
        return False
    
    # Calculate how many bytes to remove (keep only 2 tape marks = 8 bytes)
    extra_tms = tm_count - 2
    bytes_to_remove = extra_tms * 4
    
    if dry_run:
        return True
    
    new_data = data[:-bytes_to_remove]
    
    out_path = output or filepath
    with open(out_path, 'wb') as f:
        f.write(new_data)
    
    return True


def main():
    parser = argparse.ArgumentParser(
        description='Fix SIMH tape images with extra tape marks at end',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog='''
Examples:
  %(prog)s --check tape.tap           Check if file needs fixing
  %(prog)s tape.tap                   Fix file in place
  %(prog)s tape.tap -o fixed.tap      Fix to new file
  %(prog)s --check *.tap              Check multiple files
  %(prog)s *.tap                      Fix multiple files in place
'''
    )
    
    parser.add_argument('files', nargs='+', type=Path,
                        help='Tape image file(s) to check/fix')
    parser.add_argument('-o', '--output', type=Path,
                        help='Output file (only valid with single input file)')
    parser.add_argument('-c', '--check', action='store_true',
                        help='Check files only, do not modify')
    parser.add_argument('-v', '--verbose', action='store_true',
                        help='Verbose output')
    
    args = parser.parse_args()
    
    if args.output and len(args.files) > 1:
        print("Error: --output can only be used with a single input file", file=sys.stderr)
        sys.exit(1)
    
    exit_code = 0
    files_checked = 0
    files_needing_fix = 0
    files_fixed = 0
    
    for filepath in args.files:
        if not filepath.exists():
            print(f"Error: {filepath} not found", file=sys.stderr)
            exit_code = 1
            continue
        
        files_checked += 1
        needs_fix, tm_count = check_file(filepath)
        
        if args.check:
            # Check mode
            if needs_fix:
                files_needing_fix += 1
                print(f"{filepath}: NEEDS FIX - {tm_count} trailing tape marks (expected 2)")
                exit_code = 1
            elif args.verbose:
                print(f"{filepath}: OK - {tm_count} trailing tape marks")
        else:
            # Fix mode
            if needs_fix:
                files_needing_fix += 1
                output = args.output if args.output else None
                
                if fix_file(filepath, output):
                    files_fixed += 1
                    out_name = output or filepath
                    extra = tm_count - 2
                    print(f"{filepath}: Fixed - removed {extra} extra tape mark(s) -> {out_name}")
            elif args.verbose:
                print(f"{filepath}: OK - no fix needed")
    
    # Summary for multiple files
    if len(args.files) > 1:
        print()
        if args.check:
            print(f"Checked {files_checked} files: {files_needing_fix} need fixing")
        else:
            print(f"Checked {files_checked} files: {files_fixed} fixed, "
                  f"{files_needing_fix - files_fixed} failed")
    
    sys.exit(exit_code)


if __name__ == '__main__':
    main()
