import { test } from 'node:test';
import assert from 'node:assert/strict';
import { recipeSheet, validateRecipe, suggestedName, preview } from '../ui/recipes.js';

test('preview: first line, at most 80 characters', () => {
  assert.equal(preview('Pull all repos to dev\nthen migrate'), 'Pull all repos to dev');
  assert.equal(preview('x'.repeat(100)), `${'x'.repeat(79)}…`);
});

test('sheet model: recipes, suggestions with counts, empty state', () => {
  const s = recipeSheet({
    recipes: [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations' }],
    suggestions: [{ text: 'switch to main and pull, then raise a PR for the current branch', count: 9, last: 1 }],
  });
  assert.deepEqual(s.recipes, [{ id: 'r1', name: 'Fresh start', prompt: 'Pull all repos to dev, run migrations', preview: 'Pull all repos to dev, run migrations' }]);
  assert.equal(s.suggestions[0].line, 'You typed this 9 times');
  assert.equal(s.suggestions[0].preview, 'switch to main and pull, then raise a PR for the current branch');
  assert.equal(s.empty, false);
  assert.equal(recipeSheet({ recipes: [], suggestions: [] }).empty, true);
});

test('validation mirrors the backend', () => {
  assert.equal(validateRecipe({ name: ' ', prompt: 'x' }), 'Give the recipe a name.');
  assert.equal(validateRecipe({ name: 'n', prompt: '' }), 'Write the prompt to start with.');
  assert.equal(validateRecipe({ name: 'n'.repeat(61), prompt: 'x' }), 'Keep the name under 60 characters.');
  assert.equal(validateRecipe({ name: 'n', prompt: 'x'.repeat(8001) }), 'Keep the prompt under 8,000 characters.');
  assert.equal(validateRecipe({ name: 'Fresh start', prompt: 'Pull' }), null);
});

test('suggested name: first few words, capitalised', () => {
  assert.equal(suggestedName('switch to main and pull, then raise a PR'), 'Switch to main and');
  assert.equal(suggestedName('  run   tests  '), 'Run tests');
});
