from distutils.core import setup

setup(name='Knut Server',
      description='A kind helper for your home.',
      author='Joe Pearson',
      author_email='pearjo@protonmail.com',
      scripts=['scripts/knut.py'],
      packages=['knutapis',
                'knutcore',
                'knutservices',
                'knutserver'])
