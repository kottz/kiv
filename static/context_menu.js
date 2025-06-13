// static/context_menu.js

document.addEventListener('DOMContentLoaded', () => {
    const contextMenu = document.getElementById('context-menu');
    const fileBrowser = document.getElementById('file-browser');

    // Helper to generate placeholder ID based on path (must match Rust logic)
    function getTargetPlaceholderId(path) {
        if (!path) return null;
        // Replace non-alphanumeric characters (excluding hyphen) with underscore
        // Added 'g' flag for global replace, just in case.
        const itemIdBase = path.replace(/[^a-zA-Z0-9-]/g, "_");
        return `share-placeholder-${itemIdBase}`;
    }

    // --- Function to Hide Context Menu ---
    function hideContextMenu() {
        if (contextMenu && contextMenu.style.display !== 'none') {
            // console.log("Hiding context menu."); // Uncomment for debugging
            contextMenu.style.display = 'none';
        }
    }

    // --- Show Context Menu on Right-Click ---
    fileBrowser.addEventListener('contextmenu', (event) => {
        const targetLi = event.target.closest('li[data-path][id]'); // File/Dir List Item
        if (targetLi) {
            event.preventDefault();

            const path = targetLi.getAttribute('data-path');
            const isDir = targetLi.getAttribute('data-is-dir') === 'true';
            const shareTargetLi = document.getElementById('context-share-target'); // The parent LI
            const shareButtonWrapper = document.getElementById('context-share-button-wrapper'); // The inner SPAN

            // --- Basic structural check ---
            if (!shareTargetLi || !shareButtonWrapper) {
                console.error("Context menu structure elements missing!");
                hideContextMenu(); // Hide if structure is broken
                return;
            }

            // --- Clear ALL previous inline share links ---
            // Ensures only one share box is visible at a time
            // console.log("Clearing all .share-link-placeholder divs"); // Uncomment for debugging
            document.querySelectorAll('.share-link-placeholder').forEach(ph => {
                ph.innerHTML = '';
            });

            // --- Logic for files: Ensure button exists and is configured ---
            if (!isDir) {
                // Recreate button HTML inside the wrapper span to ensure it's fresh
                // and HTMX attributes are correctly defined before processing
                const buttonHTML = `<button id="context-share"
                                            hx-post="/share"
                                            hx-trigger="click"
                                            hx-target="#context-share-button-wrapper"
                                            hx-swap="innerHTML"
                                            hx-vals="">
                                       🔗 Share File
                                    </button>`;
                shareButtonWrapper.innerHTML = buttonHTML;

                // Find the newly created button
                const shareButton = shareButtonWrapper.querySelector('#context-share');

                if (shareButton) {
                    // Set the dynamic path value
                    shareButton.setAttribute('hx-vals', `{"path": "${path}"}`);

                    // IMPORTANT: Ensure HTMX processes this newly added element
                    htmx.process(shareButtonWrapper);

                } else {
                    console.error("Error: Failed to find #context-share button after recreating it.");
                }
                // Make sure the LI containing the share button is visible
                shareTargetLi.style.display = '';

                // --- Logic for directories: Hide the share option ---
            } else {
                shareTargetLi.style.display = 'none'; // Hide the whole LI
                shareButtonWrapper.innerHTML = ''; // Clear any button remnants
            }

            // --- Position and show context menu ---
            // Calculate position relative to the document, including scroll offsets
            const menuTop = event.clientY + window.scrollY;
            const menuLeft = event.clientX + window.scrollX;

            contextMenu.style.top = `${menuTop}px`;
            contextMenu.style.left = `${menuLeft}px`;
            contextMenu.style.display = 'block';

        } else {
            // Click was not on a valid file/dir list item
            hideContextMenu();
        }
    });


    // --- Hide Menu on Standard Actions (Click Outside, Scroll, Escape) ---
    document.addEventListener('click', (event) => {
        // If menu is visible AND click is outside menu AND click is outside any inline share box...
        if (contextMenu.style.display === 'block' &&
            !contextMenu.contains(event.target) &&
            !event.target.closest('.share-link-inline-box')) {
            hideContextMenu();
        }
    });
    // Use capture phase for scroll to catch it early and hide the menu
    window.addEventListener('scroll', hideContextMenu, true);
    document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') {
            hideContextMenu();
            // Also clear placeholders on Escape for good measure
            document.querySelectorAll('.share-link-placeholder').forEach(ph => {
                ph.innerHTML = '';
            });
        }
    });

    // --- Hide Menu Immediately on Share Button Click (using direct listener) ---
    // Attach listener directly to the context menu element for reliability
    contextMenu.addEventListener('click', function(event) {
        // Check if the actual clicked element or its parent is the share button
        const shareButtonClicked = event.target.closest('#context-share');
        if (shareButtonClicked) {
            // console.log("Share button clicked inside context menu, hiding menu."); // Uncomment for debugging
            hideContextMenu(); // Hide immediately, no timeout needed
        }
        
    });


    // --- Auto-select Text After Swap ---
    // This remains useful for the inline share box input
    htmx.on('htmx:afterSwap', function(evt) {
        // Check the target of the swap (the placeholder div)
        if (evt.detail.target && evt.detail.target.classList.contains('share-link-placeholder')) {
            const input = evt.detail.target.querySelector('input[type="text"]');
            if (input) {
                input.select();
            }
        }
    });

    // Ensure copy_link.js handles clicks on .copy-button (loaded separately via HTML)
});
