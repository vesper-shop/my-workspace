<script lang="ts">
	import { Button } from "@my-workspace/ui";
	import type { CommandResponse } from "@/lib/contracts/command";
	import type { Account, StoredConfiguration } from "@/lib/contracts/settings";

	let {
		compact,
		account,
		avatar,
		onswitchaccount,
	}: {
		compact: boolean;
		account: StoredConfiguration<Account> | null;
		avatar: string;
		onswitchaccount: (command: "sign_in" | "sign_out") => Promise<CommandResponse<string>>;
	} = $props();

	let open = $state(false);
	let pending = $state<"sign_in" | "sign_out" | null>(null);
	let error = $state<string | null>(null);
	let anchor = $state<HTMLDivElement | null>(null);
	const signedIn = $derived(account?.status === "ready" ? account.data : null);

	function close() {
		open = false;
		error = null;
	}

	async function switchAccount(command: "sign_in" | "sign_out") {
		pending = command;
		error = null;
		const response = await onswitchaccount(command);
		pending = null;
		if (response.status === "failed") {
			open = true;
			error = response.message;
		} else close();
	}
</script>

<svelte:document onpointerdown={(event) => { if (open && event.target instanceof Node && !anchor?.contains(event.target)) close(); }} />
<svelte:window onkeydown={(event) => { if (open && event.key === "Escape") close(); }} />

<div class="anchor" class:compact bind:this={anchor} onfocusout={(event) => { if (event.relatedTarget instanceof Node && !anchor?.contains(event.relatedTarget)) close(); }}>
	{#if open}
		<div class="menu" role="dialog" aria-label="Vesper account">
			<div class="heading">
				<img src={avatar} alt="" />
				{#if signedIn !== null}
					<div><strong>{signedIn.name}</strong><span>{signedIn.email}</span></div>
				{:else}
					<div><strong>Vesper</strong><span>One sign-in opens Memos, Moment and Knowledge.</span></div>
				{/if}
			</div>
			{#if signedIn !== null}
				<Button variant="outline" size="xs" disabled={pending !== null} onclick={() => void switchAccount("sign_out")}>{pending === "sign_out" ? "Signing out…" : "Sign out"}</Button>
			{:else}
				<Button size="xs" disabled={pending !== null || account === null} onclick={() => void switchAccount("sign_in")}>{pending === "sign_in" ? "Waiting for Vesper…" : "Sign in with Vesper"}</Button>
			{/if}
			{#if error !== null}<p role="alert">{error}</p>{/if}
		</div>
	{/if}
	<button
		class="profile"
		type="button"
		onclick={() => { if (open) close(); else open = true; }}
		aria-haspopup="dialog"
		aria-expanded={open}
		aria-label={signedIn === null ? "Sign in with Vesper" : `${signedIn.name}, ${signedIn.email}`}
		title={signedIn === null ? "Sign in with Vesper" : signedIn.email}
	>
		<img src={avatar} alt="" />
		<span class="identity">
			{#if signedIn !== null}
				<strong>{signedIn.name}</strong>
				<small>{signedIn.email}</small>
			{:else}
				<strong>Sign in</strong>
				<small>Vesper</small>
			{/if}
		</span>
	</button>
</div>

<style>
	.anchor {
		position: relative;
		display: flex;
		flex: 1 1 auto;
		min-width: 0;
	}

	.profile {
		display: flex;
		flex: 1 1 auto;
		align-items: center;
		gap: 0.375rem;
		min-width: 0;
		min-height: 2rem;
		padding: 0.2rem 0.125rem;
		border: 0;
		border-radius: var(--radius-md);
		background: transparent;
		cursor: pointer;
		text-align: left;
	}

	.profile:hover { background: var(--color-muted); }

	.profile img,
	.heading img {
		flex: 0 0 auto;
		border: 1px solid var(--color-border);
		border-radius: var(--radius-full);
		object-fit: cover;
	}

	.profile img { width: 1.75rem; height: 1.75rem; }

	.identity {
		display: grid;
		min-width: 0;
	}

	.identity strong,
	.identity small {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.identity strong {
		color: var(--color-foreground);
		font-size: 0.65rem;
		font-weight: 500;
	}

	.identity small {
		color: var(--color-muted-foreground);
		font-size: 0.58rem;
	}

	.menu {
		position: absolute;
		bottom: calc(100% + 0.75rem);
		left: 0;
		z-index: 30;
		display: grid;
		width: min(17rem, calc(100vw - 2rem));
		box-sizing: border-box;
		gap: 0.625rem;
		padding: 0.75rem;
		border: 1px solid var(--color-border);
		border-radius: var(--radius-lg);
		background: var(--color-background);
		box-shadow: var(--shadow-lg);
	}

	.heading { display: flex; align-items: center; gap: 0.625rem; }
	.heading img { width: 2.5rem; height: 2.5rem; }
	.heading div { display: grid; gap: 0.1rem; min-width: 0; }
	.heading strong { font-size: 0.75rem; font-weight: 600; }
	.heading span { color: var(--color-muted-foreground); font-size: 0.65rem; overflow-wrap: anywhere; }
	.menu p { margin: 0; color: var(--color-error); font-size: 0.62rem; }

	@media (min-width: 768px) {
		.compact .identity { display: none; }
		.compact .profile { justify-content: center; }
		.compact .menu {
			left: calc(100% + 0.75rem);
			bottom: 0;
		}
	}
</style>
