#!/usr/bin/env python3
"""Every `skill:` that workflows.yml names has its skills/<name>/SKILL.md, in the step shape."""
from pathlib import Path
import sys
import tempfile
import unittest

HOME = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(HOME / 'libexec/vendor'))
import yaml  # noqa: E402  (vendored PyYAML)


def steps(workflows):
    return yaml.safe_load(Path(workflows).read_text())['steps']


def frontmatter(path):
    text = path.read_text()
    if not text.startswith('---\n'):
        return {}
    return yaml.safe_load(text.split('---\n', 2)[1]) or {}


def problems(workflows, skills_dir):
    """One line per step whose skill file is missing or has the wrong frontmatter."""
    found = []
    for step, d in steps(workflows).items():
        name = d['skill'].split(':', 1)[-1]
        path = Path(skills_dir) / name / 'SKILL.md'
        if not path.is_file():
            found.append(f'{step}: no {path}')
            continue
        meta = frontmatter(path)
        if meta.get('name') != name:
            found.append(f'{step}: {path} has name {meta.get("name")!r}, not {name!r}')
        if not str(meta.get('description', '')).startswith(f'Workflow step `{step}` — '):
            found.append(f'{step}: {path} description does not start with "Workflow step `{step}` — "')
        if f'Use when `ramify step` names {step} as the next step' not in str(meta.get('description', '')):
            found.append(f'{step}: {path} description does not say when `ramify step` names it')
    return found


class StepSkills(unittest.TestCase):
    workflows = HOME / 'workflows.yml'
    skills = HOME / 'skills'

    def skill_text(self, step):
        return (self.skills / steps(self.workflows)[step]['skill'].split(':', 1)[-1] / 'SKILL.md').read_text()

    def test_every_workflow_skill_has_its_skill_file(self):
        self.assertEqual([], [p for p in problems(self.workflows, self.skills) if ': no ' in p])

    def test_skill_name_matches_its_folder(self):
        self.assertEqual([], [p for p in problems(self.workflows, self.skills) if 'has name' in p])

    def test_step_skill_description_names_its_step(self):
        self.assertEqual([], [p for p in problems(self.workflows, self.skills) if 'description' in p])

    def test_missing_skill_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad = Path(tmp) / 'workflows.yml'
            bad.write_text('steps:\n  nothing: { skill: ramify:nothing, does: x }\n'
                           '  spec: { skill: ramify:spec, does: x }\n')
            found = problems(bad, self.skills)
        self.assertEqual(1, len(found))
        self.assertIn('nothing: no ', found[0])

    def test_gated_skills_ask_and_wait(self):
        for step, d in steps(self.workflows).items():
            if d.get('gate') != 'human':
                continue
            with self.subTest(step=step):
                text = self.skill_text(step)
                for f in d.get('files', [f'{step}.md']):
                    self.assertIn(f'`{f}`', text)
                self.assertIn(f'ramify step ask {step}', text)
                self.assertIn(f'ramify step wait {step}', text)
                self.assertIn(f'ramify step done {step} --approved', text)
                self.assertIn('**human gate**', text)

    def test_auto_skills_record_done(self):
        for step, d in steps(self.workflows).items():
            if d.get('gate') == 'human' or step == 'finale':  # finale removes the record it would write to
                continue
            with self.subTest(step=step):
                self.assertIn(f'ramify step done {step}', self.skill_text(step))


if __name__ == '__main__': unittest.main()
