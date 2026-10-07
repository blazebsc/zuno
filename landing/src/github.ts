import { useEffect, useState } from "react";

const REPO_API = "https://api.github.com/repos/noFAYZ/zuno";

export interface Contributor {
  login: string;
  avatar: string;
  url: string;
}

export interface RepoStats {
  stars: number;
  forks: number;
  contributors: Contributor[];
}

async function getJson<T>(url: string, signal: AbortSignal): Promise<T> {
  const response = await fetch(url, { headers: { Accept: "application/vnd.github+json" }, signal });
  if (!response.ok) throw new Error(`GitHub returned HTTP ${response.status}`);
  return (await response.json()) as T;
}

async function fetchRepoStats(signal: AbortSignal): Promise<RepoStats> {
  const [repo, people] = await Promise.all([
    getJson<{ stargazers_count?: number; forks_count?: number }>(REPO_API, signal),
    // Contributors are a nice-to-have; a failure here should not cost the star count.
    getJson<Array<{ login: string; avatar_url: string; html_url: string; type: string }>>(
      `${REPO_API}/contributors?per_page=24`,
      signal,
    ).catch(() => []),
  ]);
  return {
    stars: repo.stargazers_count ?? 0,
    forks: repo.forks_count ?? 0,
    contributors: people
      .filter((person) => person.type === "User")
      .map((person) => ({ login: person.login, avatar: `${person.avatar_url}&s=80`, url: person.html_url })),
  };
}

/** Live repo numbers; null until they arrive, and for good if GitHub says no (rate limit, offline). */
export function useRepoStats(): RepoStats | null {
  const [stats, setStats] = useState<RepoStats | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    fetchRepoStats(controller.signal).then(setStats, () => {});
    return () => controller.abort();
  }, []);
  return stats;
}

export function formatCount(count: number): string {
  if (count < 1000) return String(count);
  const thousands = count / 1000;
  return `${thousands < 10 ? thousands.toFixed(1).replace(/\.0$/, "") : Math.round(thousands)}k`;
}
