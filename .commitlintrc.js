module.exports = {
  extends: ['@commitlint/config-conventional'],
  rules: {
    // Allow longer headers for descriptive commit messages
    'header-max-length': [2, 'always', 100],
    // Be case-insensitive for subjects
    'subject-case': [0],
  },
  ignores: [
    // Ignore merge commits
    (message) => message.includes('Merge pull request'),
    (message) => message.includes('Merge branch'),
    // Ignore GitHub's auto-generated commit messages
    (message) => message.startsWith('Create '),
    (message) => message.startsWith('Update '),
    (message) => message.startsWith('Delete '),
    (message) => message.startsWith('Initial commit'),
  ]
};
