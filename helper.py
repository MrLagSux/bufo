#!/usr/bin/env python3
from typing import List
import subprocess
import os
import sys
import time

from multiprocessing import Pool
from dataclasses import dataclass
from enum import Enum

COMPILER_PATH = "./target/debug/bufo"

def format_red(s: str) -> str: return f"\x1b[91m{s}\x1b[0m"
def format_yellow(s: str) -> str: return f"\x1b[93m{s}\x1b[0m"
def format_green(s: str) -> str: return f"\x1b[92m{s}\x1b[0m"

CORRUPT: str = format_red("CORRUPT")
PANIC: str = format_red("PANIC")
FAIL: str = format_red("FAIL")
IGNORE: str = format_yellow("IGNORE")
PASS: str = format_green("PASS")

def compare(expected: List[str], actual: List[str]) -> bool:
    for line in expected:
        inside = False
        for other in actual:
            if line in other:
                inside = True
                break
        if not inside:
            return False
    return True


def call_cmd(cmd: List) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(cmd, capture_output=True)

class STATE(Enum):
    SUCCESS = 0
    FAILURE = 1
    PANIC = 2
    CORRUPT = 3
    IGNORED = 4
    DONT_TEST = 5

@dataclass
class TestResult:
    path: str
    success: STATE

# TODO: Failed or panicked tests lost their output since the refactor.
#       We capture stdout and stderr, but don't print it anymore.
#       This should be the the default case, however it would be cool
#       to have a CLI arg like --show-output that then prints the output.
#       Similarly, --exit-first-failure should also show the output.
def run_test(path: str, exec: bool) -> TestResult:
    """
    The protocol for tests is as follows:
    //! THIS IS A TEST PROGRAM
    //! {RUNTIME|COMPILER}
    //! {FAILURE|SUCCESS|DIAGNOSTICS}
    //! CODE: <error code> (only if FAILURE)
    //! ERROR: (only if FAILURE)
    //! <error message> (only if FAILURE)
    //! <error message> (only if FAILURE)
    //! <...>
    //! WARNING: (only if DIAGNOSTICS)
    //! <warning message> (only if DIAGNOSTICS)
    //! <warning message> (only if DIAGNOSTICS)
    //! <...>
    <mandatory newline>
    <code>
    """
    with open(path, "r") as f:
        lines = f.readlines()
        if lines[0].startswith("//! IGNORE"):
            return TestResult(path, STATE.DONT_TEST)
        if not lines[0].startswith("//! THIS IS A TEST PROGRAM"):
            print(f"{CORRUPT} {path}", file=sys.stderr)
            return TestResult(path, STATE.CORRUPT)

        point_of_failure = lines[1].removeprefix("//! ").upper().strip()
        if point_of_failure not in ["RUNTIME", "COMPILER"]:
            print(f"{CORRUPT} {path}", file=sys.stderr)
            return TestResult(path, STATE.CORRUPT)

        if point_of_failure == "RUNTIME" and not exec:
            print(f"{IGNORE} {path}")
            return TestResult(path, STATE.IGNORED)

        expected_mode = lines[2].removeprefix("//! ").upper().strip()
        if expected_mode not in ["FAILURE", "SUCCESS", "DIAGNOSTICS"]:
            print(f"{CORRUPT} {path}", file=sys.stderr)
            return TestResult(path, STATE.CORRUPT)

        if expected_mode == "FAILURE":
            if not lines[3].startswith("//! CODE: "):
                print(f"{CORRUPT} {path}", file=sys.stderr)
                return TestResult(path, STATE.CORRUPT)
            expected_error_code = int(lines[3].removeprefix("//! CODE: ").strip())
        else:
            expected_error_code = 0
        
        error_lines = []
        warn_lines = []
        if expected_mode == "FAILURE":
            if not lines[4].startswith("//! ERROR:"):
                print(f"{CORRUPT} {path}", file=sys.stderr)
                return TestResult(path, STATE.CORRUPT)
            index = 5
            while index < len(lines) and lines[index].startswith("//! "):
                error_lines.append(lines[index].removeprefix("//! ").strip())
                index += 1
        elif expected_mode == "DIAGNOSTICS":
            if not lines[3].startswith("//! WARNING:"):
                print(f"{CORRUPT} {path}", file=sys.stderr)
                return TestResult(path, STATE.CORRUPT)
            index = 4
            while index < len(lines) and lines[index].startswith("//! "):
                warn_lines.append(lines[index].removeprefix("//! ").strip())
                index += 1
            if len(warn_lines) == 0:
                print(f"{CORRUPT} {path}", file=sys.stderr)
                return TestResult(path, STATE.CORRUPT)
        else:
            pass

        filename = "./out/{}.exe".format(path.replace(os.sep, "."))
        output = call_cmd([COMPILER_PATH, "-i", path, "-vd", "-o", filename])
        if point_of_failure == "RUNTIME":
            if output.returncode == 101:
                print(f"{PANIC} {path}", file=sys.stderr)
                return TestResult(path, STATE.PANIC)
            if output.returncode != 0:
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
            output = call_cmd([filename])
            os.remove(filename)
        if expected_mode == "DIAGNOSTICS":
            if output.returncode == 101:
                print(f"{PANIC} {path}", file=sys.stderr)
                return TestResult(path, STATE.PANIC)
            if output.returncode != 0:
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
            os.remove(filename)

        # stdout = output.stdout.decode("utf-8").split('\n')
        stderr = output.stderr.decode("utf-8").split('\n')
        if expected_mode == "FAILURE":
            if not compare(error_lines, stderr):
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
            if output.returncode != expected_error_code:
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
        elif expected_mode == "SUCCESS":
            if output.returncode == 101:
                print(f"{PANIC} {path}", file=sys.stderr)
                return TestResult(path, STATE.PANIC)
            if output.returncode != expected_error_code:
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
        elif expected_mode == "DIAGNOSTICS":
            if not compare(warn_lines, stderr):
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
            if output.returncode == 101:
                print(f"{PANIC} {path}", file=sys.stderr)
                return TestResult(path, STATE.PANIC)
            if output.returncode != expected_error_code:
                print(f"{FAIL} {path}", file=sys.stderr)
                return TestResult(path, STATE.FAILURE)
        print(f"{PASS} {path}")
        return TestResult(path, STATE.SUCCESS)

def recompile_compiler(trace: bool = False) -> None:
    print("Recompiling compiler...")
    cargo = ["cargo", "build"]
    cargo = cargo + ["--features=trace"] if trace else cargo
    cmd = call_cmd(cargo)
    if cmd.returncode != 0:
        print("Failed to recompile compiler", file=sys.stderr)
        print(cmd.stderr.decode("utf-8"), file=sys.stderr)
        sys.exit(1)
    print("Recompilation successful")

def run_all_tests(specific_tests: List[str], exec: bool = True, exit_first_failure: bool = False):
    start_time = time.time()
    total = 0
    failed_tests = []
    panicked_tests = []
    corrupt_tests = []
    ignored_tests = []
    all_tests = []
    if len(specific_tests) == 0:
        for root, _, files in os.walk("./tests"):
            for filename in files:
                path = os.path.join(root, filename)
                if os.path.isfile(path) and path.endswith(".bufo"):
                    all_tests.append(path)
    else:
        for root, _, files in os.walk("./tests"):
            for filename in files:
                path = os.path.join(root, filename)
                for t in specific_tests:
                    if f"{os.sep}{t}{os.sep}" in path and os.path.isfile(path) and path.endswith(".bufo"):
                        all_tests.append(path)
                        break # Any file can only be in one directory

    if exit_first_failure:
        for path in all_tests:
            result = run_test(path, exec)
            total += 1
            match result.success:
                case STATE.SUCCESS:
                    pass
                case STATE.FAILURE:
                    failed_tests.append(result.path)
                    break
                case STATE.PANIC:
                    panicked_tests.append(result.path)
                    break
                case STATE.CORRUPT:
                    corrupt_tests.append(result.path)
                    break
                case STATE.IGNORED:
                    ignored_tests.append(result.path)
                case STATE.DONT_TEST:
                    total -= 1
    else:
        with Pool(16) as p:
            results = p.starmap(run_test, [(path, exec) for path in all_tests])
            for result in results:
                total += 1
                match result.success:
                    case STATE.SUCCESS:
                        pass
                    case STATE.FAILURE:
                        failed_tests.append(result.path)
                    case STATE.PANIC:
                        panicked_tests.append(result.path)
                    case STATE.CORRUPT:
                        corrupt_tests.append(result.path)
                    case STATE.IGNORED:
                        ignored_tests.append(result.path)
                    case STATE.DONT_TEST:
                        total -= 1
    end_time = time.time()

    def print_tests(tests: List[str], s: str) -> None:
        if len(tests) > 0:
            print(f"\n{s} tests:")
            for test in tests:
                print(test)

    print_tests(ignored_tests, format_yellow("Ignored"))
    print_tests(failed_tests, format_red("Failed"))
    print_tests(panicked_tests, format_red("Panicked"))
    print_tests(corrupt_tests, format_red("Corrupted"))
    ignored = len(ignored_tests)
    failure = len(failed_tests)
    panicked = len(panicked_tests)
    corrupt = len(corrupt_tests)
    success = total - failure - panicked - corrupt - ignored
    print(f"\nTotal: {total}, Success: {success}, Failure: {failure}, Corrupt: {corrupt}, Panicked: {panicked}, Ignored: {ignored}")
    print(f"Time taken: {end_time - start_time:.2f} seconds")
    if failure > 0 or panicked > 0 or corrupt > 0:
        sys.exit(1)

def print_usage_and_help() -> None:
    print("Usage: python helper.py [test|bench]")
    print("Flags for test mode:")
    print("  --no-exec            -> skip running runtime tests")
    print("  --trace              -> enable tracing in the compiler (useful for debugging)")
    print("  --exit-first-failure -> exit after the first failure, disables parallelism")
    print("Flags for bench mode:")
    print("  Not implemented")

if __name__ == "__main__":
    if len(sys.argv) == 1:
        print_usage_and_help()
        exit(1)
    else:
        mode = sys.argv[1]
        if mode == "help":
            print_usage_and_help()
            exit(0)
        if mode == "test":
            trace = "--trace" in sys.argv
            recompile_compiler(trace=trace)
            print("Running tests...")
            no_exec = "--no-exec" in sys.argv
            exit_first_failure = "--exit-first-failure" in sys.argv
            test_dirs: List[str] = []
            for arg in sys.argv[2:]:
                if not arg.startswith("-"):
                    test_dirs.append(arg)
            run_all_tests(specific_tests=test_dirs, exec=not no_exec, exit_first_failure=exit_first_failure)
        elif mode == "bench":
            recompile_compiler()
            print("Running benchmarks...")
            print(format_red("Not implemented"))
            exit(1)
        else:
            print("Invalid mode: " + mode)
            print_usage_and_help()
            exit(1)
    # main()
