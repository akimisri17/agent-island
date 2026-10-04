// Recipes: named kick-off prompts per repository, and suggestions from
// prompts typed again and again there.

export function preview(text) {
  const first = String(text).split('\n')[0].trim();
  return first.length > 80 ? `${first.slice(0, 79)}…` : first;
}

export function recipeSheet({ recipes, suggestions }) {
  return {
    recipes: recipes.map((r) => ({ ...r, preview: preview(r.prompt) })),
    suggestions: suggestions.map((s) => ({ text: s.text, preview: preview(s.text), line: `You typed this ${s.count} times` })),
    empty: !recipes.length && !suggestions.length,
  };
}

// The same limits the backend enforces, checked before sending.
export function validateRecipe({ name, prompt }) {
  const n = name.trim();
  const p = prompt.trim();
  if (!n) return 'Give the recipe a name.';
  if (!p) return 'Write the prompt to start with.';
  if ([...n].length > 60) return 'Keep the name under 60 characters.';
  if ([...p].length > 8000) return 'Keep the prompt under 8,000 characters.';
  return null;
}

export function suggestedName(text) {
  const words = text.trim().split(/\s+/).slice(0, 4).join(' ').replace(/[,.;:]$/, '');
  return words.charAt(0).toUpperCase() + words.slice(1);
}
