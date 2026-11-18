<script lang="ts">
	import Tile from '#lib/ui/Tile.svelte';
</script>

<svelte:head><title>Privacy · PencilPilot</title></svelte:head>

<div class="pp-page">
	<span class="pp-eyebrow">Plain-language privacy</span>
	<h1>Privacy</h1>
	<div class="pp-bento">
		<Tile span={2}>
			<h2>What we store</h2>
			<ul>
				<li>
					<b>Without an account:</b> nothing on our server. Your practice history stays in this browser
					(local storage).
				</li>
				<li>
					<b>With an account:</b> your email, an optional display name, your password as a one-way hash
					(Argon2id; we can’t read it), and your practice history: scores, which tips you got, drill results,
					and small thumbnails of your last 24 drawings.
				</li>
				<li>
					<b>Google sign-in:</b> your Google account id, email (if verified) and name. We never see your
					Google password.
				</li>
				<li>
					<b>Sign-in cookie:</b> one cookie that keeps you signed in for 30 days. No tracking or advertising
					cookies.
				</li>
			</ul>
		</Tile>
		<Tile span={2}>
			<h2>What we don’t do</h2>
			<ul>
				<li>No ads, no selling or sharing your data.</li>
				<li>No AI looks at your drawings: scoring is plain image maths, in your browser.</li>
				<li>
					Your full drawings never leave your browser; only small thumbnails are saved to your
					account.
				</li>
			</ul>
		</Tile>
		<Tile span={2} tone="lilac">
			<h2>Your controls</h2>
			<ul>
				<li><a href="/account/export">Download all your data</a> as a file, any time.</li>
				<li>
					Delete your account on the <a href="/account">account page</a>: everything is removed
					immediately.
				</li>
			</ul>
		</Tile>
		<Tile span={2}>
			<h2>Reference pictures</h2>
			<p>
				References come from Pixabay under the Pixabay Content License and are credited on every
				page. Pictures are checked automatically for safety, and anyone can report one.
			</p>
		</Tile>
	</div>
</div>

<style>
	h1 {
		font-size: clamp(40px, 6vw, 64px);
		line-height: 0.95;
		margin: 4px 0 16px;
	}
	h2 {
		font-size: 22px;
		font-weight: 700;
	}
	ul {
		margin: 0;
		padding-left: 20px;
		display: flex;
		flex-direction: column;
		gap: 8px;
		line-height: 1.5;
	}
	p {
		margin: 0;
		line-height: 1.5;
	}
</style>
