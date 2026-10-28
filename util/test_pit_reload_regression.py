"""Prove named device failures on old parity logic, then restore and pass.

Run only in a quiescent checkout. CI owns its checkout; no live product is changed.
"""
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PIT = ROOT / 'crates/lib/marty_core/src/devices/pit.rs'
COMMAND = ['cargo', 'test', '--locked', '-p', 'marty_core', '--no-default-features',
           '--lib', 'square_wave_reload_tests']
EXPECTED = {
    'mode3_partial_even_reload_preserves_running_odd_half_cycle',
    'mode3_complete_even_reload_does_not_wrap_the_running_odd_count',
    'mode3_partial_even_reload_preserves_running_high_adjustment',
}


def run():
    result = subprocess.run(COMMAND, cwd=ROOT, capture_output=True, text=True)
    print(result.stdout, end='')
    print(result.stderr, end='')
    return result


if __name__ == '__main__':
    positive = PIT.read_bytes()
    text = positive.decode('utf8')
    pattern = (r'let even_count = if self\.ptype == PitType::Model8253 \{\s*'
               r'\(\*self\.counting_element & 1\) == 0\s*\}\s*else \{\s*'
               r'\(\*self\.count_register & 1\) == 0\s*\};')
    mutant, count = re.subn(pattern, 'let even_count = (*self.count_register & 1) == 0;', text)
    if count != 1:
        raise SystemExit('refuse: expected exactly one parity decision')
    try:
        PIT.write_text(mutant, encoding='utf8', newline='\n')
        print('DELIBERATE MUTANT: restore original count-register parity decision', flush=True)
        result = run()
        failed = set(re.findall(r'test \S+::(\w+) \.\.\. FAILED', result.stdout))
        if result.returncode != 101 or failed != EXPECTED or '2 passed; 3 failed' not in result.stdout:
            raise SystemExit('refuse: old parity must fail exactly the three named controls')
    finally:
        PIT.write_bytes(positive)
        if PIT.read_bytes() != positive:
            raise SystemExit('refuse: positive source restore failed')
    print('RESTORED POSITIVE: exact source bytes; require all five device tests', flush=True)
    result = run()
    if result.returncode or '5 passed; 0 failed' not in result.stdout:
        raise SystemExit('refuse: restored positive device suite failed')
