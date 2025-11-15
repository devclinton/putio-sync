# Local Testing Instructions for OAuth Flow

## Prerequisites

- Rust toolchain installed (1.70+)
- A Put.io account
- OAuth application registered with Put.io

## Step 1: Register OAuth Application with Put.io

1. Go to Put.io's OAuth app registration page (check Put.io docs for URL)
2. Register a new application with:
   - **Application Name**: `putio-sync` (or your preferred name)
   - **Redirect URI**: `http://localhost:29876/auth/callback`
3. Save your **Client ID** and **Client Secret**

## Step 2: Configure Environment

Create a `.env` file in the project root:

```bash
# Copy the example file
cp .env.example .env

# Edit .env with your credentials
nano .env  # or use your preferred editor
```

Your `.env` should look like:

```env
PUTIO_CLIENT_ID=your_actual_client_id
PUTIO_CLIENT_SECRET=your_actual_client_secret
PUTIO_REDIRECT_URI=http://localhost:29876/auth/callback
```

⚠️ **Important**: Never commit `.env` to version control! It's already in `.gitignore`.

## Step 3: Build the Binary

```bash
# Build release binary (optimized)
cargo build --release

# Binary will be at:
# ./target/release/putio-sync
```

## Step 4: Test OAuth Flow

### Basic OAuth Login (Recommended)

```bash
# Run get-token command with auto-save
./target/release/putio-sync get-token --save
```

**What happens:**
1. Starts local HTTP server on port 29876
2. Opens browser to Put.io authorization page
3. You click "Authorize"
4. Put.io redirects back to `http://localhost:29876/auth/callback`
5. App exchanges code for access token
6. Verifies token with Put.io API
7. Displays account info
8. Saves token to `~/.putiosync/config.toml`

### OAuth Flow Without Auto-Save

```bash
# Get token but don't save it yet
./target/release/putio-sync get-token

# Token will be displayed in terminal
# You can then use it with:
./target/release/putio-sync login --token YOUR_TOKEN_HERE
```

### OAuth Flow Without Browser Opening

```bash
# Don't open browser automatically (useful for SSH/remote)
./target/release/putio-sync get-token --no-browser --save

# You'll need to manually visit the authorization URL
# Copy and paste it in your local browser
```

## Step 5: Verify It Works

```bash
# List your Put.io files
./target/release/putio-sync list

# Expected output: table of your files
```

## Testing App-Specific Password (Alternative Auth Method)

If you also want to test app-specific password authentication:

### Step 1: Generate App-Specific Password

1. Go to https://app.put.io/settings/account
2. Find "App-Specific Passwords" section
3. Click "Generate New Password"
4. Give it a name: "putio-sync-testing"
5. Copy the generated password

### Step 2: Login with Password

```bash
./target/release/putio-sync login \
  --username your@email.com \
  --password YOUR_APP_SPECIFIC_PASSWORD
```

### Step 3: Verify

```bash
./target/release/putio-sync list
```

## Troubleshooting

### Error: "Failed to load .env file"

**Problem**: The `.env` file is missing or in the wrong location.

**Solution**: Make sure `.env` exists in the project root:
```bash
# Check if .env exists
ls -la .env

# If not, create it from example
cp .env.example .env

# Edit with your credentials
nano .env
```

### Error: "PUTIO_CLIENT_ID not found in .env file"

**Problem**: `.env` file is empty or missing required variables.

**Solution**: Ensure all three variables are set:
```env
PUTIO_CLIENT_ID=your_client_id
PUTIO_CLIENT_SECRET=your_client_secret
PUTIO_REDIRECT_URI=http://localhost:29876/auth/callback
```

### Error: "Failed to bind to address"

**Problem**: Port 29876 is already in use.

**Solutions**:
1. **Option A**: Kill the process using port 29876:
   ```bash
   # Find process
   lsof -i :29876

   # Kill it
   kill -9 PID
   ```

2. **Option B**: Use a different port:
   ```env
   # In .env, change to different port
   PUTIO_REDIRECT_URI=http://localhost:8080/auth/callback
   ```

   ⚠️ Remember to update your OAuth app's redirect URI in Put.io settings!

### Error: "Authorization failed" or "CSRF token mismatch"

**Problem**: OAuth flow was interrupted or state token didn't match.

**Solution**: Try again. If it persists:
```bash
# Clear any cached tokens
rm ~/.putiosync/config.toml

# Try again
./target/release/putio-sync get-token --save
```

### Browser Opens But Redirects to Wrong Port

**Problem**: Redirect URI in `.env` doesn't match OAuth app settings.

**Solution**: Ensure they match exactly:
- **.env**: `PUTIO_REDIRECT_URI=http://localhost:29876/auth/callback`
- **Put.io OAuth app**: Redirect URI should be `http://localhost:29876/auth/callback`

### Can't Open Browser (SSH/Remote Session)

**Solution**: Use `--no-browser` flag:
```bash
./target/release/putio-sync get-token --no-browser --save

# Copy the URL and open it in your local browser
```

## Verifying Your Setup

### Check .env File

```bash
# View .env (be careful not to share output!)
cat .env

# Should show:
# PUTIO_CLIENT_ID=...
# PUTIO_CLIENT_SECRET=...
# PUTIO_REDIRECT_URI=http://localhost:29876/auth/callback
```

### Check Config File After Login

```bash
# View saved config
cat ~/.putiosync/config.toml

# Should show:
# [auth]
# auth_method = "Token"
# token = "YOUR_ACCESS_TOKEN"
```

### Test All Commands

```bash
# List files
./target/release/putio-sync list

# List with filters
./target/release/putio-sync list --filter "*.mp4"

# List with long format
./target/release/putio-sync list --long

# Different output formats
./target/release/putio-sync list --output json
./target/release/putio-sync list --output csv
./target/release/putio-sync list --output tree
```

## Example Session

Here's what a successful OAuth flow looks like:

```bash
$ ./target/release/putio-sync get-token --save

=== Put.io OAuth Token Helper ===

This command will help you obtain an OAuth token from Put.io using OAuth 2.0.

Step 1: Starting local callback server
-------------------------------
Starting server on http://localhost:29876/auth/callback...
✓ Server started successfully!

Step 2: Authorize putio-sync
-------------------------------
Opening browser to Put.io authorization page...
✓ Browser opened successfully!

Waiting for authorization...
✓ Authorization received!

Step 3: Exchanging code for access token
-------------------------------
✓ Access token received!

Step 4: Verifying token
-------------------------------
✓ Token verified successfully!

Account Information:
  Username: yourname
  Email: your@email.com

Step 5: Saving token
-------------------------------
✓ Token saved to: /Users/you/.putiosync/config.toml

✓ All done! You can now use putio-sync commands.

$ ./target/release/putio-sync list
┌────────────┬─────────────────┬──────────┬────────────┬─────────────────────┐
│ ID         │ Name            │ Type     │ Size       │ Created             │
├────────────┼─────────────────┼──────────┼────────────┼─────────────────────┤
│ 123456789  │ Movies          │ folder   │ -          │ 2024-01-15 10:30:00 │
│ 987654321  │ video.mp4       │ video    │ 1.23 GB    │ 2024-01-20 14:45:00 │
└────────────┴─────────────────┴──────────┴────────────┴─────────────────────┘
```

## Security Notes

1. **Never commit `.env` to git** - It contains secrets!
2. **Keep client secret private** - Don't share it
3. **Access tokens expire** - You may need to re-authenticate
4. **Use `--no-verify` carefully** - Only for testing invalid credentials

## Switching Between Auth Methods

### From OAuth Token to App-Specific Password

```bash
# Currently using OAuth token
./target/release/putio-sync list  # works

# Switch to app-specific password
./target/release/putio-sync login \
  --username your@email.com \
  --password APP_PASSWORD

# Now using app-specific password
./target/release/putio-sync list  # still works
```

### Back to OAuth Token

```bash
# Use get-token to get a fresh token
./target/release/putio-sync get-token --save
```

## Clean Up

### Remove Test Tokens

```bash
# Remove config
rm ~/.putiosync/config.toml

# Start fresh
./target/release/putio-sync get-token --save
```

### Remove OAuth App (Optional)

If you want to start over:
1. Go to Put.io OAuth apps page
2. Delete your test application
3. Create a new one
4. Update `.env` with new credentials

## Next Steps

Once you've verified OAuth flow works:

1. **Commit the implementation** (but not `.env`!)
2. **Update README** with OAuth setup instructions
3. **Add to documentation** how to register OAuth app
4. **Test on different platforms** (macOS, Linux, Windows)

## Quick Reference

```bash
# Setup
cp .env.example .env
nano .env  # add credentials

# Build
cargo build --release

# OAuth login
./target/release/putio-sync get-token --save

# Test
./target/release/putio-sync list

# Alternative: app-specific password
./target/release/putio-sync login -u email -p password
```

That's it! You now have a fully working OAuth 2.0 flow for putio-sync! 🎉
