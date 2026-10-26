"""PreToolUse hook: never let Claude read, edit or print .env files.

Permission deny rules only cover the Read/Edit tools, so shell commands like
`cat .env` or `Get-Content .env` need this extra check. env_keys_names.txt and
*.example files stay allowed: they hold variable names, not secrets.
"""
import json
import os
import re
import sys

ENV_NAME = re.compile(r'^(\.env(\..+)?|.+\.env)$', re.IGNORECASE)
ENV_IN_COMMAND = re.compile(r'(^|[\s/\\\'"=(:])(\.env(\.[\w.-]+)?|[\w.-]+\.env)(?=$|[\s\'"|;&>)<,])', re.IGNORECASE)
# Runtime env objects in code (`process.env`, `import.meta.env`), not files on disk.
CODE_ENV_ACCESS = re.compile(r'(^|\.)(process|meta)\.env$', re.IGNORECASE)


def is_secret_name(name):
	return bool(ENV_NAME.match(name)) and not name.lower().endswith('.example')


def main():
	try:
		data = json.load(sys.stdin)
	except ValueError:
		return 0
	tool_input = data.get('tool_input') or {}

	for key in ('file_path', 'path', 'notebook_path'):
		value = tool_input.get(key)
		if value and is_secret_name(os.path.basename(str(value).rstrip('/\\'))):
			return block(value)

	for key in ('command', 'glob', 'pattern'):
		value = tool_input.get(key)
		if not value:
			continue
		for match in ENV_IN_COMMAND.finditer(str(value)):
			name = match.group(2)
			if CODE_ENV_ACCESS.search(name):
				continue
			if is_secret_name(name):
				return block(name)
	return 0


def block(target):
	print(
		f'Blocked: {target} contains secrets and must not be opened (see CLAUDE.md). '
		'Variable names are listed in env_keys_names.txt.',
		file=sys.stderr,
	)
	return 2


if __name__ == '__main__':
	sys.exit(main())
