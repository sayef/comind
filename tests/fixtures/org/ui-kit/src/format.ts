export function formatDate(d: Date): string {
  return d.toISOString();
}

export class Formatter {
  format(d: Date): string {
    return formatDate(d);
  }
}
