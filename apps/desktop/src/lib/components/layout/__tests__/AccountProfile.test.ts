import { afterEach, expect, it, vi } from "vite-plus/test";
import { mount, tick, unmount } from "svelte";
import type { CommandResponse } from "@/lib/contracts/command";
import AccountProfile from "../AccountProfile.svelte";

const cleanups: Array<() => Promise<void>> = [];
afterEach(async () => {
	for (const cleanup of cleanups.splice(0)) await cleanup();
	document.body.replaceChildren();
});

function container() {
	const target = document.createElement("div");
	document.body.append(target);
	return target;
}

function action(target: HTMLElement, label: string) {
	const found = Array.from(target.querySelectorAll("button")).find(
		(button) => button.textContent?.trim() === label,
	);
	if (!found) throw new Error(`Button ${label} missing`);
	return found;
}

async function openMenu(target: HTMLElement) {
	const profile = target.querySelector(".profile");
	if (!(profile instanceof HTMLButtonElement)) throw new Error("Profile missing");
	profile.click();
	await tick();
}

it("waits for the stored account before offering sign-in", async () => {
	const target = container();
	const view = mount(AccountProfile, {
		target,
		props: { compact: false, account: null, avatar: "avatar.png", onswitchaccount: vi.fn() },
	});
	cleanups.push(() => unmount(view));
	await openMenu(target);
	expect(action(target, "Sign in with Vesper").disabled).toBe(true);
	expect(target.querySelector(".identity")?.textContent).toContain("Sign in");
});

it("keeps the menu open with the failure when sign-in fails", async () => {
	const target = container();
	const onswitchaccount = vi
		.fn()
		.mockResolvedValue({ status: "failed", message: "Browser closed" });
	const view = mount(AccountProfile, {
		target,
		props: {
			compact: false,
			account: { status: "missing" },
			avatar: "avatar.png",
			onswitchaccount,
		},
	});
	cleanups.push(() => unmount(view));
	await openMenu(target);
	action(target, "Sign in with Vesper").click();
	await vi.waitFor(() =>
		expect(target.querySelector('[role="alert"]')?.textContent).toBe("Browser closed"),
	);
	expect(onswitchaccount).toHaveBeenCalledWith("sign_in");
	expect(target.querySelector('[role="dialog"]')).not.toBeNull();
});

it("shows the Vesper identity and signs out", async () => {
	const target = container();
	const onswitchaccount = vi.fn().mockResolvedValue({ status: "ready", data: "account" });
	const view = mount(AccountProfile, {
		target,
		props: {
			compact: false,
			account: {
				status: "ready",
				data: { email: "@owner", name: "Owner", picture: "https://example.com/owner.png" },
			},
			avatar: "https://example.com/owner.png",
			onswitchaccount,
		},
	});
	cleanups.push(() => unmount(view));
	expect(target.querySelector(".identity strong")?.textContent).toBe("Owner");
	expect(target.querySelector(".identity small")?.textContent).toBe("@owner");
	await openMenu(target);
	expect(target.querySelector('[role="dialog"] img')?.getAttribute("src")).toBe(
		"https://example.com/owner.png",
	);
	action(target, "Sign out").click();
	await vi.waitFor(() => expect(target.querySelector('[role="dialog"]')).toBeNull());
	expect(onswitchaccount).toHaveBeenCalledWith("sign_out");
});

it("closes on an outside click or Escape and reopens to show a late failure", async () => {
	const target = container();
	let settle: (response: CommandResponse<string>) => void = () => {
		throw new Error("Sign-in has not started");
	};
	const onswitchaccount = vi.fn(
		() => new Promise<CommandResponse<string>>((resolve) => (settle = resolve)),
	);
	const view = mount(AccountProfile, {
		target,
		props: {
			compact: false,
			account: { status: "missing" },
			avatar: "avatar.png",
			onswitchaccount,
		},
	});
	cleanups.push(() => unmount(view));
	await openMenu(target);
	document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
	await tick();
	expect(target.querySelector('[role="dialog"]')).toBeNull();
	await openMenu(target);
	window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
	await tick();
	expect(target.querySelector('[role="dialog"]')).toBeNull();
	await openMenu(target);
	action(target, "Sign in with Vesper").click();
	document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
	await tick();
	expect(target.querySelector('[role="dialog"]')).toBeNull();
	settle({ status: "failed", message: "Browser closed" });
	await vi.waitFor(() =>
		expect(target.querySelector('[role="alert"]')?.textContent).toBe("Browser closed"),
	);
});
