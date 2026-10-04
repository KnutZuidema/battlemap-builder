/** mm:ss for the elapsed-time readouts while a model call is running. */
export function formatTimer(sec) {
  const m = Math.floor(sec / 60);
  const s = sec % 60;
  return `${m < 10 ? '0' : ''}${m}:${s < 10 ? '0' : ''}${s}`;
}

/** Formats USD cost e.g. "$0.0712" or "$0.00" */
export function formatCost(cost, digits = 4) {
  const num = typeof cost === 'number' && !isNaN(cost) ? cost : 0;
  if (num === 0) return '$0.00';
  if (num < 0.0001) return '<$0.0001';
  return `$${num.toFixed(digits)}`;
}

/** Formats token numbers with commas e.g. 1,234 */
export function formatTokens(num) {
  if (typeof num !== 'number' || isNaN(num)) return '0';
  return num.toLocaleString();
}
