document.addEventListener('DOMContentLoaded', () => {
    // Use event delegation on the body or a closer persistent container
    // like #share-result-area if you prefer
    document.body.addEventListener('click', async (event) => {
        // Check if the clicked element is a copy button
        if (event.target.matches('.copy-button')) {
            const button = event.target;
            const targetSelector = button.getAttribute('data-copy-target');
            if (!targetSelector) {
                console.error('Copy button missing data-copy-target attribute');
                return;
            }

            const inputElement = document.querySelector(targetSelector);
            if (!inputElement) {
                console.error('Target input element not found:', targetSelector);
                return;
            }

            const textToCopy = inputElement.value;

            try {
                await navigator.clipboard.writeText(textToCopy);
                // Provide feedback
                button.textContent = 'Copied!';
                button.disabled = true; // Optional: disable after copy

                // Reset button text after a short delay
                setTimeout(() => {
                    button.textContent = 'Copy';
                    button.disabled = false; // Re-enable if disabled
                }, 2000); // Reset after 2 seconds

            } catch (err) {
                console.error('Failed to copy text: ', err);
                // Optionally provide error feedback to the user
                button.textContent = 'Error';
                setTimeout(() => {
                    button.textContent = 'Copy';
                }, 2000);
            }
        }
    });
});
