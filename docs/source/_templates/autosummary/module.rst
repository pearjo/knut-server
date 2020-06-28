{{ name | escape | underline }}

.. automodule:: {{ fullname | escape }}

{% if classes %}
.. autosummary::
    :toctree: .

    {% for class in classes %}
        {{ class }}
    {% endfor %}

{% endif %}
